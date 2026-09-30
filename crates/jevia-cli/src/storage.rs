//! Backend-independent run storage. JSONL remains the backwards-compatible default.
//! SQL keeps the complete versioned record; indexed columns are derived, never authority.

mod database;
mod import;
#[cfg(test)]
mod tests;

pub use database::ObservationIndexStatus;
pub use import::validate_import;

use crate::{lease, paths::ProjectPaths, store};
use anyhow::{Context, Result, bail};
use database::Database;
use jevia_core::{
    Config, ExecutionEvidence, HarnessObservations, Outcome, RouteRecord, RunState, StorageConfig,
};
use std::{fs::File, path::PathBuf};

pub enum Storage {
    Jsonl(ProjectPaths),
    Database(Database),
}

// Keep the lock alive through both harness execution and its verifier. Database
// guards own a detached connection so a session lock is never returned to a pool.
pub enum ExecutionGuard {
    File { _file: lease::FileLock },
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
            Self::Jsonl(paths) => store::get(&paths.runs, id),
            Self::Database(db) => db.get(id).await,
        }
    }

    /// Best-effort recovery reads skip locked JSONL history rather than waiting.
    pub async fn get_for_replay(&self, id: &str) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => {
                let path = paths.runs.clone();
                let id = id.to_owned();
                tokio::task::spawn_blocking(move || store::try_get(&path, &id)).await?
            }
            Self::Database(db) => db.get(id).await,
        }
    }

    /// Replay workers must stop scanning even if their async waiter was dropped.
    pub async fn get_for_replay_until(
        &self,
        id: &str,
        deadline: std::time::Instant,
    ) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => {
                let path = paths.runs.clone();
                let id = id.to_owned();
                tokio::task::spawn_blocking(move || {
                    store::try_get_until(&path, &id, Some(deadline))
                })
                .await?
            }
            Self::Database(db) => db.get(id).await,
        }
    }

    /// Separate windows prevent passive activity from displacing known outcomes.
    /// Each window retains append order; the provider consumes them separately.
    pub async fn routing_history(&self, limit: usize) -> Result<Vec<RouteRecord>> {
        let (mut history, observations) = match self {
            Self::Jsonl(paths) => store::routing_history(&paths.runs, limit)?,
            Self::Database(db) => db.routing_history(limit).await?,
        };
        // Both windows share one snapshot. Preserve de-duplication for legacy
        // JSONL files containing repeated identities; deep check diagnoses those.
        for record in observations {
            if !history
                .iter()
                .any(|known| known.decision.run_id == record.decision.run_id)
            {
                history.push(record);
            }
        }
        Ok(history)
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

    pub async fn complete_external(
        &self,
        id: &str,
        outcome: Outcome,
        reason: Option<&str>,
        confirmed_stopped: bool,
    ) -> Result<RouteRecord> {
        if !confirmed_stopped {
            bail!(
                "external completion requires --confirm-stopped after all external work and verification have stopped"
            );
        }
        let _guard = self.execution_guard(id).await?;
        match self {
            Self::Jsonl(paths) => store::complete_external(&paths.runs, id, outcome, reason),
            Self::Database(db) => db.complete_external(id, outcome, reason).await,
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

    pub async fn record_application(
        &self,
        id: &str,
        input: jevia_core::ExecutionRecording,
    ) -> Result<RouteRecord> {
        let _guard = self.execution_guard(id).await?;
        match self {
            Self::Jsonl(paths) => store::record_application(&paths.runs, id, input),
            Self::Database(db) => db.record_application(id, input).await,
        }
    }

    /// The caller holds the execution lease. A supervisor must still own its SQL run.
    pub async fn checkpoint_observations(
        &self,
        id: &str,
        observations: HarnessObservations,
        supervisor: bool,
    ) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => {
                let path = paths.runs.clone();
                let id = id.to_owned();
                // No synchronous file I/O on the process supervisor's task. The
                // worker never waits for a busy history lock; the journal is the
                // durable retry source. A late supervisor write rejects terminal runs.
                tokio::task::spawn_blocking(move || {
                    store::checkpoint_observations(&path, &id, observations, supervisor)
                })
                .await?
            }
            Self::Database(db) => {
                db.checkpoint_observations(id, observations, supervisor)
                    .await
            }
        }
    }

    pub async fn checkpoint_for_replay(
        &self,
        id: &str,
        observations: HarnessObservations,
        deadline: std::time::Instant,
    ) -> Result<RouteRecord> {
        match self {
            Self::Jsonl(paths) => {
                let path = paths.runs.clone();
                let id = id.to_owned();
                tokio::task::spawn_blocking(move || {
                    store::checkpoint_observations_until(
                        &path,
                        &id,
                        observations,
                        false,
                        Some(deadline),
                    )
                })
                .await?
            }
            Self::Database(db) => db.checkpoint_observations(id, observations, false).await,
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
            Self::Jsonl(paths) => store::count(&paths.runs),
            Self::Database(db) => db.check().await,
        }
    }

    /// Read-only performance metadata; never initialize, backfill, or repair.
    pub async fn observation_index_status(&self) -> Result<ObservationIndexStatus> {
        match self {
            Self::Jsonl(_) => Ok(ObservationIndexStatus::NotApplicable),
            Self::Database(db) => db.observation_index_status().await,
        }
    }

    /// Inspect logical history integrity without a write probe or automatic repair.
    pub async fn check_deep(&self) -> Result<usize> {
        match self {
            Self::Jsonl(paths) => store::check_deep(&paths.runs),
            Self::Database(db) => db.check_deep().await,
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
        use std::{
            fs,
            io::{BufWriter, Write},
        };
        // Fail before reading history, including for dangling destination symlinks.
        // persist_noclobber below still handles a destination created during export.
        match fs::symlink_metadata(output) {
            Ok(_) => bail!("could not create export; existing files are never overwritten"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("could not inspect export destination"),
        }
        let parent = output.parent().context("export path has no parent")?;
        let mut file = tempfile::Builder::new()
            .prefix(".jevia-export-")
            .suffix(".tmp")
            .tempfile_in(parent)?;
        let count = {
            let mut writer = BufWriter::new(file.as_file_mut());
            let count = match self {
                Self::Jsonl(paths) => store::export(&paths.runs, &mut writer)?,
                Self::Database(db) => db.export(&mut writer).await?,
            };
            writer
                .flush()
                .context("could not flush export; destination was not created")?;
            count
        };
        file.as_file().sync_all()?;
        file.persist_noclobber(output)
            .map_err(|e| e.error)
            .context("could not create export; existing files are never overwritten")?;
        #[cfg(unix)]
        File::open(parent).and_then(|directory| directory.sync_all()).with_context(|| format!("export was created at {} but its directory could not be synced; inspect the file before retrying", output.display()))?;
        Ok(count)
    }

    pub async fn import_jsonl(&self, source: PathBuf, apply: bool) -> Result<(usize, usize)> {
        let Self::Database(db) = self else {
            bail!("import-jsonl requires a configured SQLite or PostgreSQL destination");
        };
        let snapshot = import::Snapshot::capture(&source)?;
        db.import(snapshot.records(), apply).await
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
        db.import(records.iter().cloned().map(Ok), apply).await
    }
}
