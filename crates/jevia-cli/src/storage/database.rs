use std::{fs, future::Future, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use jevia_core::{
    ExecutionEvidence, Outcome, RECORD_SCHEMA_VERSION, RouteRecord, RunState, StorageConfig,
};
use sha2::{Digest, Sha256};
use sqlx::{
    Any, AnyPool, ConnectOptions, Connection, Row, Transaction,
    any::{AnyConnectOptions, AnyPoolOptions},
};

use super::ExecutionGuard;
use crate::{lease, paths::ProjectPaths, store};

const SCHEMA_VERSION: i64 = 1;
const DB_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Database {
    pool: AnyPool,
    project: String,
    sqlite_path: Option<PathBuf>,
    // Every CLI invocation has a distinct token. Recovery revokes the previous
    // token, fencing delayed writes from a disconnected supervisor.
    owner: String,
}

// Driver errors can contain connection strings, passwords, task text, or server
// details. Never retain them as an anyhow source, even for alternate formatting.
async fn db<T>(future: impl Future<Output = sqlx::Result<T>>) -> Result<T> {
    tokio::time::timeout(DB_TIMEOUT, future).await
        .map_err(|_| anyhow!("database operation timed out; history was not redirected to local storage"))?
        .map_err(|_| anyhow!("database operation failed; check connectivity, permissions and schema (driver details redacted)"))
}

impl Database {
    pub async fn open(
        config: &StorageConfig,
        paths: &ProjectPaths,
        initialize: bool,
    ) -> Result<Self> {
        sqlx::any::install_default_drivers();
        let (url, project, sqlite_path) = match config {
            StorageConfig::Sqlite { url } => {
                let file = paths.root.join(
                    url.strip_prefix("sqlite://")
                        .context("invalid SQLite URL")?,
                );
                let parent = file.parent().context("SQLite path has no parent")?;
                if initialize {
                    fs::create_dir_all(parent)?;
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    match options.open(&file) {
                        Ok(file) => file.sync_all()?,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e).context("could not create SQLite file"),
                    }
                }
                if !file.is_file() {
                    bail!("SQLite database is missing; run `jevia storage init`");
                }
                let file = fs::canonicalize(file)?;
                let uri = url::Url::from_file_path(&file)
                    .map_err(|_| anyhow!("invalid SQLite file path"))?;
                if uri.host_str().is_some() {
                    bail!("SQLite requires a local disk; network shares are not supported");
                }
                // file:///C:/... contains a URL-only leading slash. SQLite
                // needs the native drive path on Windows, not /C:/....
                #[cfg(windows)]
                let filename = uri.path().strip_prefix('/').unwrap_or(uri.path());
                #[cfg(not(windows))]
                let filename = uri.path();
                let url = format!("sqlite://{filename}?mode=rw");
                (url, "local".to_owned(), Some(file))
            }
            StorageConfig::Postgres {
                url_env,
                project,
                allow_insecure_localhost,
            } => {
                let raw = std::env::var(url_env)
                    .map_err(|_| anyhow!("storage database URL environment variable is missing"))?;
                (
                    postgres_url(&raw, *allow_insecure_localhost)?,
                    project.clone(),
                    None,
                )
            }
            StorageConfig::Jsonl => unreachable!("JSONL does not open a database"),
        };
        let options = AnyConnectOptions::from_str(&url)
            .map_err(|_| anyhow!("invalid database connection options (details redacted)"))?
            .disable_statement_logging();
        let postgres = sqlite_path.is_none();
        let pool = db(AnyPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(DB_TIMEOUT)
            .after_connect(move |connection, _| {
                Box::pin(async move {
                    if postgres {
                        sqlx::query("SET statement_timeout = '5s'")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("SET lock_timeout = '5s'")
                            .execute(&mut *connection)
                            .await?;
                    } else {
                        sqlx::query("PRAGMA busy_timeout = 5000")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA foreign_keys = ON")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA journal_mode = WAL")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA synchronous = FULL")
                            .execute(&mut *connection)
                            .await?;
                    }
                    Ok(())
                })
            })
            .connect_with(options))
        .await?;
        let database = Self {
            pool,
            project,
            sqlite_path,
            owner: uuid::Uuid::new_v4().to_string(),
        };
        if initialize {
            database.initialize().await?;
        }
        database.validate_schema().await?;
        // A missing project must never silently create a new history on reads.
        let exists: Option<i64> = db(sqlx::query_scalar(
            "SELECT next_seq FROM jevia_projects WHERE project = $1",
        )
        .bind(&database.project)
        .fetch_optional(&database.pool))
        .await?;
        if exists.is_none() {
            bail!("database project is not initialized; run `jevia storage init`");
        }
        Ok(database)
    }

    pub fn name(&self) -> &'static str {
        if self.is_postgres() {
            "postgres"
        } else {
            "sqlite"
        }
    }
    pub fn is_postgres(&self) -> bool {
        self.sqlite_path.is_none()
    }

    #[cfg(test)]
    pub(super) async fn set_schema_version_for_test(&self, version: i64) {
        sqlx::query("UPDATE jevia_schema SET version = $1")
            .bind(version)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn initialize(&self) -> Result<()> {
        let mut tx = db(self.pool.begin()).await?;
        if self.is_postgres() {
            db(sqlx::query("SELECT pg_advisory_xact_lock(7416920201101)").execute(&mut *tx))
                .await?;
        }
        db(sqlx::query("CREATE TABLE IF NOT EXISTS jevia_schema (id BIGINT PRIMARY KEY, version BIGINT NOT NULL)")
            .execute(&mut *tx)).await?;
        db(sqlx::query(
            "INSERT INTO jevia_schema (id, version) VALUES (1, $1) ON CONFLICT (id) DO NOTHING",
        )
        .bind(SCHEMA_VERSION)
        .execute(&mut *tx))
        .await?;
        let version: i64 = db(
            sqlx::query_scalar("SELECT version FROM jevia_schema WHERE id = 1").fetch_one(&mut *tx),
        )
        .await?;
        if version != SCHEMA_VERSION {
            bail!("unsupported database schema version; database was not migrated");
        }
        db(sqlx::query("CREATE TABLE IF NOT EXISTS jevia_projects (project TEXT PRIMARY KEY, next_seq BIGINT NOT NULL)")
            .execute(&mut *tx)).await?;
        db(sqlx::query("CREATE TABLE IF NOT EXISTS jevia_runs (
            project TEXT NOT NULL REFERENCES jevia_projects(project), run_id TEXT NOT NULL,
            ordinal BIGINT NOT NULL, learning BIGINT NOT NULL, record TEXT NOT NULL,
            owner TEXT NOT NULL DEFAULT '', PRIMARY KEY(project, run_id), UNIQUE(project, ordinal))")
            .execute(&mut *tx)).await?;
        db(sqlx::query("CREATE INDEX IF NOT EXISTS jevia_runs_evidence ON jevia_runs(project, learning, ordinal)")
            .execute(&mut *tx)).await?;
        db(sqlx::query("INSERT INTO jevia_projects (project, next_seq) VALUES ($1, 0) ON CONFLICT (project) DO NOTHING")
            .bind(&self.project).execute(&mut *tx)).await?;
        db(tx.commit()).await
    }

    async fn validate_schema(&self) -> Result<()> {
        let version: i64 = db(
            sqlx::query_scalar("SELECT version FROM jevia_schema WHERE id = 1")
                .fetch_one(&self.pool),
        )
        .await
        .context("storage schema unavailable; initialize with `jevia storage init`")?;
        if version != SCHEMA_VERSION {
            bail!("unsupported database schema version; use a compatible Jevia version");
        }
        Ok(())
    }

    /// Serialize short writes per project. Never held across API requests or
    /// subprocess execution. The first statement obtains SQLite's write lock,
    /// avoiding a deferred read transaction's unsafe upgrade to a write lock.
    async fn write(&self) -> Result<Transaction<'_, Any>> {
        let mut tx = db(self.pool.begin()).await?;
        let result = db(sqlx::query(
            "UPDATE jevia_projects SET next_seq = next_seq WHERE project = $1",
        )
        .bind(&self.project)
        .execute(&mut *tx))
        .await?;
        if result.rows_affected() != 1 {
            bail!("database project no longer exists");
        }
        Ok(tx)
    }

    pub async fn recent(&self, limit: usize, evidence_only: bool) -> Result<Vec<RouteRecord>> {
        let query = if evidence_only {
            "SELECT record FROM jevia_runs WHERE project = $1 AND learning = 1 ORDER BY ordinal DESC LIMIT $2"
        } else {
            "SELECT record FROM jevia_runs WHERE project = $1 ORDER BY ordinal DESC LIMIT $2"
        };
        let rows: Vec<String> = db(sqlx::query_scalar(query)
            .bind(&self.project)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&self.pool))
        .await?;
        rows.iter().rev().map(|row| decode(row)).collect()
    }

    pub async fn get(&self, id: &str) -> Result<RouteRecord> {
        let row: Option<String> = db(sqlx::query_scalar(
            "SELECT record FROM jevia_runs WHERE project = $1 AND run_id = $2",
        )
        .bind(&self.project)
        .bind(id)
        .fetch_optional(&self.pool))
        .await?;
        decode(&row.context("run id was not found in this project's history")?)
    }

    async fn insert(&self, tx: &mut Transaction<'_, Any>, record: &RouteRecord) -> Result<()> {
        validate_record(record)?;
        let ordinal: i64 = db(sqlx::query_scalar("UPDATE jevia_projects SET next_seq = next_seq + 1 WHERE project = $1 RETURNING next_seq")
            .bind(&self.project).fetch_one(&mut **tx)).await?;
        db(sqlx::query("INSERT INTO jevia_runs (project, run_id, ordinal, learning, record) VALUES ($1, $2, $3, $4, $5)")
            .bind(&self.project).bind(&record.decision.run_id).bind(ordinal)
            .bind(i64::from(record.is_learning_evidence())).bind(serde_json::to_string(record)?)
            .execute(&mut **tx)).await?;
        Ok(())
    }

    pub async fn append(&self, record: &RouteRecord) -> Result<()> {
        let mut tx = self.write().await?;
        self.insert(&mut tx, record).await?;
        db(tx.commit()).await
    }

    pub async fn outcome(
        &self,
        id: &str,
        outcome: Outcome,
        reason: Option<&str>,
    ) -> Result<RouteRecord> {
        self.mutate(id, None, false, |record| {
            store::apply_outcome(record, outcome, reason)
        })
        .await
    }

    pub async fn state(
        &self,
        id: &str,
        state: RunState,
        outcome: Outcome,
        execution: Option<ExecutionEvidence>,
        recover: bool,
    ) -> Result<RouteRecord> {
        self.mutate(id, Some(state), recover, |record| {
            store::apply_state(record, state, outcome, execution)
        })
        .await
    }

    async fn mutate(
        &self,
        id: &str,
        state: Option<RunState>,
        recover: bool,
        mutation: impl FnOnce(&mut RouteRecord) -> Result<()>,
    ) -> Result<RouteRecord> {
        let mut tx = self.write().await?;
        let row = db(sqlx::query(
            "SELECT record, owner FROM jevia_runs WHERE project = $1 AND run_id = $2",
        )
        .bind(&self.project)
        .bind(id)
        .fetch_optional(&mut *tx))
        .await?
        .context("run id was not found in this project's history")?;
        let mut record = decode(
            row.try_get("record")
                .map_err(|_| anyhow!("invalid stored record"))?,
        )?;
        let owner: String = row
            .try_get("owner")
            .map_err(|_| anyhow!("invalid stored owner"))?;
        if let Some(next) = state {
            if next == RunState::Running {
                if !owner.is_empty() {
                    bail!("run already belongs to another supervisor");
                }
            } else if !recover && owner != self.owner {
                bail!("run ownership was revoked; refusing a stale supervisor update");
            }
        }
        mutation(&mut record)?;
        record.schema_version = RECORD_SCHEMA_VERSION;
        let owner = match state {
            Some(next) if next.is_active() => self.owner.as_str(),
            Some(_) => "",
            None => owner.as_str(),
        };
        db(sqlx::query("UPDATE jevia_runs SET record = $3, learning = $4, owner = $5 WHERE project = $1 AND run_id = $2")
            .bind(&self.project).bind(id).bind(serde_json::to_string(&record)?)
            .bind(i64::from(record.is_learning_evidence())).bind(owner).execute(&mut *tx)).await?;
        db(tx.commit()).await?;
        Ok(record)
    }

    pub async fn execution_guard(&self, id: &str) -> Result<ExecutionGuard> {
        if let Some(file) = &self.sqlite_path {
            let file = lease::try_acquire(&file.with_extension("run-locks"), id)?.context(
                "run still has an active Jevia supervisor; recovery or execution refused",
            )?;
            return Ok(ExecutionGuard::File { _file: file });
        }
        // Direct/session-pooled PostgreSQL connections only. Transaction-mode
        // poolers cannot preserve a session advisory lock between statements.
        let mut connection = db(self.pool.acquire()).await?.detach();
        let key = Sha256::digest(format!("jevia-run:{}:{id}", self.project).as_bytes());
        let key = i64::from_be_bytes(key[..8].try_into().expect("eight bytes"));
        let acquired: i64 = db(sqlx::query_scalar(
            "SELECT CASE WHEN pg_try_advisory_lock($1) THEN 1::BIGINT ELSE 0::BIGINT END",
        )
        .bind(key)
        .fetch_one(&mut connection))
        .await?;
        if acquired != 1 {
            let _ = connection.close().await;
            bail!("run still has an active Jevia supervisor; recovery or execution refused");
        }
        Ok(ExecutionGuard::Postgres {
            _connection: connection,
        })
    }

    pub async fn check(&self) -> Result<usize> {
        self.validate_schema().await?;
        let mut tx = self.write().await?;
        let id = format!("check-{}", uuid::Uuid::new_v4());
        // Roll back the entire probe, including its ordering counter. This
        // validates SELECT/INSERT/UPDATE/DELETE without adding fake evidence.
        db(sqlx::query("INSERT INTO jevia_runs (project, run_id, ordinal, learning, record) VALUES ($1, $2, -1, 0, '{}')")
            .bind(&self.project).bind(&id).execute(&mut *tx)).await?;
        db(
            sqlx::query("UPDATE jevia_runs SET learning = 0 WHERE project = $1 AND run_id = $2")
                .bind(&self.project)
                .bind(&id)
                .execute(&mut *tx),
        )
        .await?;
        db(
            sqlx::query("DELETE FROM jevia_runs WHERE project = $1 AND run_id = $2")
                .bind(&self.project)
                .bind(&id)
                .execute(&mut *tx),
        )
        .await?;
        let count: i64 = db(sqlx::query_scalar(
            "SELECT COUNT(*) FROM jevia_runs WHERE project = $1",
        )
        .bind(&self.project)
        .fetch_one(&mut *tx))
        .await?;
        db(tx.rollback()).await?;
        usize::try_from(count).context("invalid record count")
    }

    pub async fn import(&self, records: &[RouteRecord], apply: bool) -> Result<(usize, usize)> {
        let mut tx = self.write().await?;
        let mut imported = 0;
        let mut skipped = 0;
        for record in records {
            let existing: Option<String> = db(sqlx::query_scalar(
                "SELECT record FROM jevia_runs WHERE project = $1 AND run_id = $2",
            )
            .bind(&self.project)
            .bind(&record.decision.run_id)
            .fetch_optional(&mut *tx))
            .await?;
            if let Some(existing) = existing {
                if decode(&existing)? != *record {
                    bail!("import conflicts with an existing run; no records imported");
                }
                skipped += 1;
            } else {
                if apply {
                    self.insert(&mut tx, record).await?;
                }
                imported += 1;
            }
        }
        if apply {
            db(tx.commit()).await?;
        } else {
            db(tx.rollback()).await?;
        }
        Ok((imported, skipped))
    }
}

fn decode(raw: &str) -> Result<RouteRecord> {
    let record: RouteRecord = serde_json::from_str(raw)
        .map_err(|_| anyhow!("invalid database record (contents redacted)"))?;
    validate_record(&record)?;
    Ok(record)
}

fn validate_record(record: &RouteRecord) -> Result<()> {
    if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
        bail!("unsupported history record schema; refusing to read or modify it");
    }
    if record.decision.run_id.is_empty() {
        bail!("run id cannot be empty");
    }
    Ok(())
}

fn postgres_url(raw: &str, insecure: bool) -> Result<String> {
    let mut url =
        url::Url::parse(raw).map_err(|_| anyhow!("invalid PostgreSQL URL (contents redacted)"))?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str().is_none()
        || url.fragment().is_some()
        || url.path().trim_matches('/').is_empty()
    {
        bail!("storage requires a PostgreSQL URL with an explicit host and database");
    }
    let loopback = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if insecure && !loopback {
        bail!("plaintext PostgreSQL is allowed only for explicit loopback development connections");
    }
    let mut pairs = Vec::new();
    let mut tls_mode = if insecure { "disable" } else { "verify-full" };
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "sslmode" if value == "verify-full" => tls_mode = "verify-full",
            "sslmode" if insecure && value == "disable" => {}
            "sslmode" => bail!(
                "PostgreSQL requires sslmode=verify-full; plaintext loopback needs allow_insecure_localhost=true"
            ),
            "sslrootcert" | "sslcert" | "sslkey" | "application_name" => {
                pairs.push((key.into_owned(), value.into_owned()))
            }
            _ => bail!(
                "unsupported PostgreSQL URL option; use host/database/credentials in the URL and TLS options in its query"
            ),
        }
    }
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(pairs)
        .append_pair("sslmode", tls_mode);
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_not_exposed_and_tls_cannot_be_downgraded() {
        assert!(
            postgres_url("postgres://user:secret@example.com/history", false)
                .unwrap()
                .contains("sslmode=verify-full")
        );
        assert!(
            postgres_url(
                "postgres://user:secret@127.0.0.1/history?sslmode=disable",
                true
            )
            .unwrap()
            .contains("sslmode=disable")
        );
        for (raw, insecure) in [
            (
                "postgres://user:secret@example.com/history?sslmode=disable",
                false,
            ),
            ("postgres://user:secret@example.com/history", true),
            (
                "postgres://user:secret@127.0.0.1/history?host=example.com",
                true,
            ),
            ("mysql://user:secret@localhost/history", false),
            ("secret-not-a-url", false),
        ] {
            let error = postgres_url(raw, insecure).unwrap_err();
            assert!(!format!("{error:#}").contains("secret"));
        }
    }

    #[tokio::test]
    async fn nested_driver_errors_are_redacted() {
        let error =
            db::<()>(async { Err(sqlx::Error::Protocol("password=secret task=private".into())) })
                .await
                .unwrap_err();
        assert!(!format!("{error:#}").contains("secret"));
        assert!(error.source().is_none());
    }
}
