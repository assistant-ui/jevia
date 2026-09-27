//! Backend-independent run storage. JSONL remains the backwards-compatible default.
//! SQL keeps the complete versioned record; indexed columns are derived, never authority.

mod database;
#[cfg(test)]
mod tests;

use crate::{lease, paths::ProjectPaths, store};
use anyhow::{Context, Result, bail};
use database::Database;
use jevia_core::{Config, ExecutionEvidence, Outcome, RouteRecord, RunState, StorageConfig};
use std::{collections::HashSet, fs::File, path::PathBuf};

pub enum Storage {
    Jsonl(ProjectPaths),
    Database(Database),
}

// Keep the lock alive through both harness execution and its verifier. Database
// guards own a detached connection so a session lock is never returned to a pool.
pub enum ExecutionGuard {
    File { _file: File },
    Postgres { _connection: sqlx::AnyConnection },
}

impl Storage {
    pub async fn open(config: &Config, paths: &ProjectPaths, initialize: bool) -> Result<Self> {
        match &config.storage {
            StorageConfig::Jsonl => Ok(Self::Jsonl(paths.clone())),
            storage => Ok(Self::Database(
                Database::open(storage, paths, initialize).await?,
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Jsonl(_) => "jsonl",
            Self::Database(db) => db.name(),
        }
    }

    pub fn requires_recovery_confirmation(&self) -> bool {
        matches!(self, Self::Database(db) if db.is_postgres())
    }

    /// Ascending append order, matching the JSONL contract (not wall-clock order).
    pub async fn recent(&self, limit: usize, evidence_only: bool) -> Result<Vec<RouteRecord>> {
        match self {
            Self::Jsonl(paths) => store::recent(&paths.runs, limit, evidence_only),
            Self::Database(db) => db.recent(limit, evidence_only).await,
        }
    }

    pub async fn get(&self, id: &str) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => store::load(&paths.runs)?
                .into_iter()
                .find(|r| r.decision.run_id == id)
                .context("run id was not found in history"),
            Self::Database(db) => db.get(id).await,
        }
    }

    pub async fn append(&self, record: &RouteRecord) -> Result<()> {
        match self {
            Self::Jsonl(paths) => store::append(&paths.runs, record),
            Self::Database(db) => db.append(record).await,
        }
    }

    pub async fn outcome(
        &self,
        id: &str,
        outcome: Outcome,
        reason: Option<&str>,
    ) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => store::update_outcome(&paths.runs, id, outcome, reason),
            Self::Database(db) => db.outcome(id, outcome, reason).await,
        }
    }

    pub async fn state(
        &self,
        id: &str,
        state: RunState,
        outcome: Outcome,
        execution: Option<ExecutionEvidence>,
    ) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => store::record_state(&paths.runs, id, state, outcome, execution),
            Self::Database(db) => db.state(id, state, outcome, execution, false).await,
        }
    }

    pub async fn recover(&self, id: &str, confirmed_stopped: bool) -> Result<RouteRecord> {
        if self.requires_recovery_confirmation() && !confirmed_stopped {
            bail!(
                "PostgreSQL recovery requires --confirm-stopped after checking the original machine and its processes; a lost connection is not proof that work stopped"
            );
        }
        let _guard = self.execution_guard(id).await?;
        match self {
            Self::Jsonl(paths) => store::record_state(
                &paths.runs,
                id,
                RunState::Interrupted,
                Outcome::Unknown,
                None,
            ),
            Self::Database(db) => {
                db.state(id, RunState::Interrupted, Outcome::Unknown, None, true)
                    .await
            }
        }
    }

    pub async fn execution_guard(&self, id: &str) -> Result<ExecutionGuard> {
        match self {
            Self::Jsonl(paths) => {
                let file = lease::try_acquire(&paths.directory.join("run-leases"), id)?.context(
                    "run still has an active Jevia supervisor; recovery or execution refused",
                )?;
                Ok(ExecutionGuard::File { _file: file })
            }
            Self::Database(db) => db.execution_guard(id).await,
        }
    }

    pub async fn check(&self) -> Result<usize> {
        match self {
            Self::Jsonl(paths) => Ok(store::load(&paths.runs)?.len()),
            Self::Database(db) => db.check().await,
        }
    }

    pub async fn archive(
        &self,
        paths: &ProjectPaths,
        keep: usize,
        apply: bool,
    ) -> Result<store::MaintenanceReport> {
        match self {
            Self::Jsonl(paths) => {
                store::maintain(&paths.runs, store::Maintenance::Archive { keep }, apply)
            }
            Self::Database(db) => db.archive(&paths.directory, keep, apply).await,
        }
    }

    /// Never overwrite history. An explicit export is also useful as a migration backup.
    pub async fn export(&self, output: &std::path::Path) -> Result<usize> {
        use std::io::Write;
        let records = self.recent(usize::MAX, false).await?;
        let parent = output.parent().context("export path has no parent")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        for record in &records {
            serde_json::to_writer(&mut file, record)?;
            file.write_all(b"\n")?;
        }
        file.as_file().sync_all()?;
        file.persist_noclobber(output)
            .map_err(|e| e.error)
            .context("could not create export; existing files are never overwritten")?;
        Ok(records.len())
    }

    pub async fn import_jsonl(&self, source: PathBuf, apply: bool) -> Result<(usize, usize)> {
        if matches!(self, Self::Jsonl(_)) {
            bail!("import-jsonl requires a configured SQLite or PostgreSQL destination");
        }
        if !source.is_file() {
            bail!("import source is not a file");
        }
        let records = store::load(&source)?;
        self.import_records(&records, apply).await
    }

    pub async fn import_records(
        &self,
        records: &[RouteRecord],
        apply: bool,
    ) -> Result<(usize, usize)> {
        let Self::Database(db) = self else {
            bail!("import-jsonl requires a configured SQLite or PostgreSQL destination");
        };
        validate_import(records)?;
        db.import(records, apply).await
    }
}

pub fn validate_import(records: &[RouteRecord]) -> Result<()> {
    let mut ids = HashSet::new();
    for record in records {
        if !ids.insert(&record.decision.run_id) {
            bail!("duplicate run id in import; no records imported");
        }
        if record
            .lifecycle
            .as_ref()
            .is_some_and(|l| l.state.is_active())
        {
            bail!("source contains active runs; stop and recover them before importing");
        }
    }
    Ok(())
}
