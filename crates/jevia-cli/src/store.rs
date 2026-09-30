use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use jevia_core::{
    ExecutionEvidence, FeedbackEvent, HarnessObservations, Outcome, OutcomeEvidence, OutcomeSource,
    RECORD_SCHEMA_VERSION, RouteRecord, RunLifecycle, RunState,
};
use tempfile::NamedTempFile;

mod check;
mod history;
mod maintenance;
pub use check::check_deep;
pub use history::routing_history;
pub(crate) use maintenance::archivable;
pub use maintenance::{Maintenance, Report as MaintenanceReport, maintain};

mod lookup;
pub use lookup::{get, try_get};

pub fn load(path: &Path) -> Result<Vec<RouteRecord>> {
    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    if !parent.exists() {
        return Ok(Vec::new());
    }

    let _lock = acquire_lock(path, LockMode::Shared)?;
    load_unlocked(path)
}

fn load_unlocked(path: &Path) -> Result<Vec<RouteRecord>> {
    let mut records = Vec::new();
    read_records(path, |record| {
        records.push(record);
        Ok(())
    })?;
    Ok(records)
}

/// Validate the full JSONL stream while retaining only the requested tail.
/// Memory scales with the window, not with the size of the retained history.
pub fn recent(path: &Path, limit: usize, evidence_only: bool) -> Result<Vec<RouteRecord>> {
    recent_matching(path, limit, |record| {
        !evidence_only || record.is_learning_evidence()
    })
}

fn recent_matching(
    path: &Path,
    limit: usize,
    eligible: impl Fn(&RouteRecord) -> bool,
) -> Result<Vec<RouteRecord>> {
    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let mut records = VecDeque::new();
    read_records(path, |record| {
        if limit != 0 && eligible(&record) {
            if records.len() == limit {
                records.pop_front();
            }
            records.push_back(record);
        }
        Ok(())
    })?;
    Ok(records.into_iter().collect())
}

/// Stream a validated export under the same shared lock used by history readers.
pub fn export(path: &Path, mut output: impl Write) -> Result<usize> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok(0);
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let mut count = 0;
    read_records(path, |record| {
        serde_json::to_writer(&mut output, &record)?;
        output.write_all(b"\n")?;
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// Read an existing import source while cooperating with Jevia's JSONL writers.
/// Unlike ordinary history reads, a missing source is never an empty import.
pub(crate) fn with_import_reader<T>(
    path: &Path,
    read: impl FnOnce(BufReader<File>) -> Result<T>,
) -> Result<T> {
    if !path.is_file() {
        bail!("import source is not a file");
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let file = File::open(path).context("could not open import source")?;
    if !file.metadata()?.is_file() {
        bail!("import source is not a file");
    }
    read(BufReader::new(file))
}

fn read_records(path: &Path, visit: impl FnMut(RouteRecord) -> Result<()>) -> Result<()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not open run history at {}", path.display()));
        }
    };

    read_records_from(BufReader::new(file), path, visit)
}

/// Parse already-captured bytes without opening a file or creating a sidecar lock.
pub fn parse_snapshot(path: &Path, bytes: &[u8]) -> Result<Vec<RouteRecord>> {
    let mut records = Vec::new();
    read_records_from(bytes, path, |record| {
        records.push(record);
        Ok(())
    })?;
    Ok(records)
}

fn read_records_from(
    reader: impl BufRead,
    path: &Path,
    mut visit: impl FnMut(RouteRecord) -> Result<()>,
) -> Result<()> {
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("could not read line {} from {}", index + 1, path.display())
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: RouteRecord = serde_json::from_str(&line)
            .map_err(|error| crate::diagnostics::json_line("run record", index + 1, &error))?;
        if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
            bail!(
                "unsupported run record schema {} on line {} of {}",
                record.schema_version,
                index + 1,
                path.display()
            );
        }
        record
            .decision
            .validate()
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "invalid routing decision on line {} (contents redacted)",
                    index + 1
                )
            })?;
        visit(record)?;
    }
    Ok(())
}

pub fn append(path: &Path, record: &RouteRecord) -> Result<()> {
    record.decision.validate().map_err(anyhow::Error::msg)?;
    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let mut encoded = serde_json::to_vec(record).context("could not encode run record")?;
    encoded.push(b'\n');

    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    let mut file = private_append_options()
        .open(path)
        .with_context(|| format!("could not open {} for writing", path.display()))?;
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            // Readers accept a valid final record without a newline. Validate
            // before extending it, so truncated/unsupported records stay intact
            // for explicit repair. The exclusive lock covers inspection and the
            // append; the normal newline-terminated path only reads one byte.
            file.rewind()?;
            read_records_from(BufReader::new(&mut file), path, |_| Ok(()))?;
            encoded.insert(0, b'\n');
        }
    }
    file.write_all(&encoded)
        .context("could not append run record")?;
    file.sync_data().context("could not sync run history")?;
    sync_parent(path)?;
    Ok(())
}

pub fn update_outcome(
    path: &Path,
    run_id: &str,
    outcome: Outcome,
    reason: Option<&str>,
) -> Result<RouteRecord> {
    update(path, run_id, |record| {
        apply_outcome(record, outcome, reason)
    })
}

pub(super) fn apply_outcome(
    record: &mut RouteRecord,
    outcome: Outcome,
    reason: Option<&str>,
) -> Result<()> {
    let reason = reason.map(str::trim).filter(|value| !value.is_empty());
    if reason.is_some_and(|value| value.len() > 4096) {
        bail!("feedback reason must be at most 4096 bytes");
    }
    if record
        .lifecycle
        .as_ref()
        .is_some_and(|life| life.state.is_active())
    {
        bail!("cannot change feedback while a run is active; inspect or recover it first");
    }
    if record.outcome != Outcome::Unknown && outcome != record.outcome && reason.is_none() {
        bail!("changing a known outcome requires --reason");
    }
    let recorded_at_ms = now_ms();
    record.feedback.push(FeedbackEvent {
        previous_outcome: record.outcome,
        previous_source: record
            .outcome_evidence
            .as_ref()
            .map(|evidence| evidence.source),
        outcome,
        recorded_at_ms,
        reason: reason.map(str::to_owned),
    });
    record.outcome = outcome;
    record.outcome_evidence = Some(OutcomeEvidence {
        source: OutcomeSource::Manual,
        recorded_at_ms,
    });
    Ok(())
}

pub fn complete_external(
    path: &Path,
    run_id: &str,
    outcome: Outcome,
    reason: Option<&str>,
) -> Result<RouteRecord> {
    update(path, run_id, |record| {
        apply_external_completion(record, outcome, reason)
    })
}

pub(super) fn apply_external_completion(
    record: &mut RouteRecord,
    outcome: Outcome,
    reason: Option<&str>,
) -> Result<()> {
    if record.execution.is_some()
        || record.lifecycle.as_ref().is_some_and(|life| {
            life.state != RunState::Routed
                || life.started_at_ms.is_some()
                || life.finished_at_ms.is_some()
        })
        || record
            .outcome_evidence
            .as_ref()
            .is_some_and(|evidence| evidence.source != OutcomeSource::Manual)
    {
        bail!(
            "only pending, externally executed runs can be completed; inspect or recover supervised runs instead"
        );
    }
    // Preserve manual provenance and correction rules. No invented harness,
    // verifier, start time, or process-exit evidence for work we did not observe.
    apply_outcome(record, outcome, reason)?;
    record.lifecycle = Some(RunLifecycle {
        state: RunState::Completed,
        started_at_ms: None,
        finished_at_ms: Some(now_ms()),
    });
    Ok(())
}

#[cfg(test)]
pub fn record_execution(
    path: &Path,
    run_id: &str,
    outcome: Outcome,
    execution: ExecutionEvidence,
) -> Result<RouteRecord> {
    record_state(path, run_id, RunState::Completed, outcome, Some(execution))
}

pub fn record_state(
    path: &Path,
    run_id: &str,
    state: RunState,
    outcome: Outcome,
    execution: Option<ExecutionEvidence>,
) -> Result<RouteRecord> {
    update(path, run_id, |record| {
        apply_state(record, state, outcome, execution)
    })
}

pub fn checkpoint_observations(
    path: &Path,
    run_id: &str,
    observations: HarnessObservations,
    supervisor: bool,
) -> Result<RouteRecord> {
    let lock = private_lock_options().open(path.with_extension("lock"))?;
    lock.try_lock()
        .context("history busy; observation journal retained for retry")?;
    let _guard = crate::lease::FileLock::new(lock);
    update_unlocked(path, run_id, |record| {
        if supervisor
            && !record
                .lifecycle
                .as_ref()
                .is_some_and(|life| life.state.is_active())
        {
            bail!("run is no longer active; refusing a stale observation checkpoint");
        }
        apply_observations(record, observations)
    })
}

pub fn record_application(
    path: &Path,
    run_id: &str,
    input: jevia_core::ExecutionRecording,
) -> Result<RouteRecord> {
    update(path, run_id, |record| apply_application(record, input))
}

/// A single immutable, caller-reported attempt. Exact retries are idempotent;
/// conflicting submissions, native executions and active runs are never overwritten.
pub(super) fn apply_application(
    record: &mut RouteRecord,
    input: jevia_core::ExecutionRecording,
) -> Result<()> {
    let execution = input.into_evidence().map_err(anyhow::Error::msg)?;
    if record.execution.as_ref() == Some(&execution)
        && record
            .lifecycle
            .as_ref()
            .is_some_and(|l| l.state == RunState::Completed)
    {
        return Ok(());
    }
    if record.execution.is_some()
        || !record
            .lifecycle
            .as_ref()
            .is_some_and(|l| l.state == RunState::Routed)
    {
        bail!(
            "execution recording requires a routed app-owned run; existing executions cannot be replaced"
        );
    }
    let now = now_ms();
    record.lifecycle = Some(RunLifecycle {
        state: RunState::Completed,
        started_at_ms: None, // Duration is caller-reported; do not invent a start timestamp.
        finished_at_ms: Some(now),
    });
    record.execution = Some(execution);
    // Preserve independently supplied manual feedback. Never create an outcome
    // or a process/verification assertion from caller-reported activity.
    Ok(())
}

/// Replace a cumulative snapshot, never append it as a new attempt or outcome.
pub(super) fn apply_observations(
    record: &mut RouteRecord,
    observations: HarnessObservations,
) -> Result<()> {
    let execution = record
        .execution
        .as_mut()
        .context("run has no execution to checkpoint")?;
    // Also validate callers constructing structs directly, not only wire input.
    let observations: HarnessObservations =
        serde_json::from_value(serde_json::to_value(observations)?)?;
    if let Some(previous) = &execution.observations {
        if previous.source != observations.source {
            bail!("observation source does not match the run");
        }
        if observations.event_count() < previous.event_count()
            || observations.counts().discarded_inputs < previous.counts().discarded_inputs
        {
            bail!("refusing an older observation snapshot");
        }
    }
    execution.observations = Some(observations);
    Ok(())
}

pub(super) fn apply_state(
    record: &mut RouteRecord,
    state: RunState,
    outcome: Outcome,
    execution: Option<ExecutionEvidence>,
) -> Result<()> {
    let now = now_ms();
    let life = record.lifecycle.get_or_insert_with(Default::default);
    if !matches!(
        life.state,
        RunState::Routed | RunState::Running | RunState::Verifying
    ) {
        bail!("run is already terminal; execution is never automatically retried");
    }
    if state == RunState::Running && life.state != RunState::Routed {
        bail!("run has already started");
    }
    if state == RunState::Interrupted && !life.state.is_active() {
        bail!("only an active execution can be recovered");
    }
    life.started_at_ms.get_or_insert(now);
    life.state = state;
    if !state.is_active() {
        life.finished_at_ms = Some(now);
    }
    record.outcome = outcome;
    if let Some(mut execution) = execution {
        if let Some(previous) = record
            .execution
            .as_ref()
            .and_then(|e| e.observations.as_ref())
        {
            let incoming = execution.observations.as_ref();
            if incoming.is_none_or(|next| {
                next.source != previous.source
                    || next.event_count() < previous.event_count()
                    || next.counts().discarded_inputs < previous.counts().discarded_inputs
            }) {
                let mut retained = previous.clone();
                if incoming
                    .is_some_and(|next| next.status == jevia_core::ObservationStatus::Partial)
                {
                    retained.status = jevia_core::ObservationStatus::Partial;
                }
                execution.observations = Some(retained);
            }
        }
        record.execution = Some(execution);
    }
    record.outcome_evidence = if state == RunState::Completed {
        let verified = record
            .execution
            .as_ref()
            .and_then(|execution| execution.verification.as_ref())
            .is_some_and(|verification| verification.launched && verification.exit_code.is_some());
        Some(OutcomeEvidence {
            source: if verified {
                OutcomeSource::Verification
            } else {
                OutcomeSource::ProcessExit
            },
            recorded_at_ms: now,
        })
    } else {
        None
    };
    Ok(())
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn update(
    path: &Path,
    run_id: &str,
    update_record: impl FnOnce(&mut RouteRecord) -> Result<()>,
) -> Result<RouteRecord> {
    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    update_unlocked(path, run_id, update_record)
}

fn update_unlocked(
    path: &Path,
    run_id: &str,
    update_record: impl FnOnce(&mut RouteRecord) -> Result<()>,
) -> Result<RouteRecord> {
    let mut records = load_unlocked(path)?;
    let updated = records
        .iter_mut()
        .find(|record| record.decision.run_id == run_id)
        .context("run id was not found in local history")?;
    update_record(updated)?;
    updated.schema_version = RECORD_SCHEMA_VERSION;
    let result = updated.clone();

    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("could not create a temporary file in {}", parent.display()))?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        for record in &records {
            serde_json::to_writer(&mut writer, record).context("could not encode run record")?;
            writer
                .write_all(b"\n")
                .context("could not terminate run record")?;
        }
        writer
            .flush()
            .context("could not flush updated run history")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("could not sync updated run history")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not atomically replace {}", path.display()))?;
    sync_parent(path)?;

    Ok(result)
}

#[derive(Debug, Clone, Copy)]
enum LockMode {
    Shared,
    Exclusive,
}

fn acquire_lock(path: &Path, mode: LockMode) -> Result<crate::lease::FileLock> {
    let lock_path = path.with_extension("lock");
    let lock = private_lock_options()
        .open(&lock_path)
        .with_context(|| format!("could not open history lock at {}", lock_path.display()))?;
    crate::file_lock::acquire(
        lock,
        matches!(mode, LockMode::Shared),
        std::time::Duration::from_secs(2),
        "run history",
    )
}

fn private_append_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).read(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options
}

fn private_lock_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    File::open(parent)
        .with_context(|| format!("could not open {} for syncing", parent.display()))?
        .sync_all()
        .with_context(|| format!("could not sync {}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Arc, Barrier},
        thread,
    };

    use jevia_core::RouteDecision;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn append_preserves_valid_unterminated_history_bytes() {
        let first = sample_record();
        let mut second = sample_record();
        second.decision.run_id = "run-2".into();
        for suffix in ["", " ", "\r", "\n", "\r\n", "\n  "] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("runs.jsonl");
            let original = format!("{}{suffix}", serde_json::to_string(&first).unwrap());
            fs::write(&path, &original).unwrap();
            assert_eq!(load(&path).unwrap(), vec![first.clone()]);
            append(&path, &second).unwrap();
            assert_eq!(load(&path).unwrap(), vec![first.clone(), second.clone()]);
            let separator = if original.ends_with('\n') { "" } else { "\n" };
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                format!(
                    "{original}{separator}{}\n",
                    serde_json::to_string(&second).unwrap()
                )
            );
        }
    }

    #[test]
    fn append_refuses_invalid_unterminated_history_without_modifying_it() {
        let mut unsupported = sample_record();
        unsupported.schema_version = RECORD_SCHEMA_VERSION + 1;
        for original in [
            "{\"schema_version\":".to_owned(),
            "not json".to_owned(),
            serde_json::to_string(&unsupported).unwrap(),
        ] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("runs.jsonl");
            fs::write(&path, &original).unwrap();
            assert!(append(&path, &sample_record()).is_err());
            assert_eq!(fs::read_to_string(path).unwrap(), original);
        }
    }

    #[test]
    fn append_handles_empty_and_whitespace_only_history() {
        for original in ["", " ", "\r", "\n"] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("runs.jsonl");
            fs::write(&path, original).unwrap();
            append(&path, &sample_record()).unwrap();
            assert_eq!(load(&path).unwrap(), vec![sample_record()]);
        }
    }

    #[test]
    fn streaming_export_matches_history_and_stops_on_write_failure() {
        struct Fails;
        impl Write for Fails {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("export writer failed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let directory = tempdir().unwrap();
        let path = directory.path().join("runs.jsonl");
        let mut out = Vec::new();
        assert_eq!(export(&path, &mut out).unwrap(), 0);
        assert!(out.is_empty());
        let mut expected = Vec::new();
        let mut original = String::new();
        for index in 0..405 {
            let mut record = sample_record();
            record.schema_version = 1 + index % 3;
            record.decision.run_id = format!("export-{index}");
            original.push_str(&serde_json::to_string(&record).unwrap());
            original.push_str("\n \n");
            expected.push(record);
        }
        fs::write(&path, original.trim_end()).unwrap(); // A valid unterminated final line is readable.
        assert_eq!(export(&path, &mut out).unwrap(), expected.len());
        assert_eq!(parse_snapshot(&path, &out).unwrap(), expected);
        assert_eq!(fs::read_to_string(&path).unwrap(), original.trim_end());
        let damaged = format!(
            "{}\n{{malformed-tail",
            serde_json::to_string(&sample_record()).unwrap()
        );
        fs::write(&path, &damaged).unwrap();
        let error = export(&path, Fails).unwrap_err();
        assert!(format!("{error:#}").contains("export writer failed")); // Writer runs before parsing the tail.
        assert!(export(&path, Vec::new()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), damaged);
    }

    #[test]
    fn streaming_export_holds_the_shared_history_lock() {
        struct LockProbe {
            path: std::path::PathBuf,
            checked: bool,
        }
        impl Write for LockProbe {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let lock = private_lock_options().open(self.path.with_extension("lock"))?;
                assert!(
                    lock.try_lock().is_err(),
                    "a writer must not enter while exporting"
                );
                self.checked = true;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let directory = tempdir().unwrap();
        let path = directory.path().join("runs.jsonl");
        append(&path, &sample_record()).unwrap();
        let mut probe = LockProbe {
            path: path.clone(),
            checked: false,
        };
        assert_eq!(export(&path, &mut probe).unwrap(), 1);
        assert!(probe.checked);
        let lock = private_lock_options()
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
    }

    #[test]
    fn bounded_recent_matches_full_history_and_filters_before_limiting() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("runs.jsonl");
        assert!(recent(&path, 10, false).unwrap().is_empty());
        for index in 0..20 {
            let mut record = sample_record();
            record.decision.run_id = format!("run-{index}");
            record.decision.created_at_ms = 20 - index;
            if index % 3 == 0 {
                record.outcome = Outcome::Success;
                record.outcome_evidence = Some(OutcomeEvidence {
                    source: OutcomeSource::Manual,
                    recorded_at_ms: 1,
                });
            }
            append(&path, &record).unwrap();
        }
        let records = load(&path).unwrap();
        for evidence_only in [false, true] {
            let selected: Vec<_> = records
                .iter()
                .filter(|r| !evidence_only || r.is_learning_evidence())
                .cloned()
                .collect();
            for limit in [0, 1, 5, 20, 30, usize::MAX] {
                assert_eq!(
                    recent(&path, limit, evidence_only).unwrap(),
                    selected[selected.len().saturating_sub(limit)..]
                );
            }
        }
    }

    #[test]
    fn lifecycle_recovers_without_claiming_a_task_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        append(&path, &sample_record()).unwrap();
        let running =
            record_state(&path, "run-1", RunState::Running, Outcome::Unknown, None).unwrap();
        assert_eq!(running.schema_version, RECORD_SCHEMA_VERSION);
        assert!(running.lifecycle.unwrap().started_at_ms.is_some());
        assert!(update_outcome(&path, "run-1", Outcome::Success, None).is_err());
        let recovered = record_state(
            &path,
            "run-1",
            RunState::Interrupted,
            Outcome::Unknown,
            None,
        )
        .unwrap();
        assert_eq!(recovered.outcome, Outcome::Unknown);
        assert!(recovered.lifecycle.unwrap().finished_at_ms.is_some());
        assert!(record_state(&path, "run-1", RunState::Running, Outcome::Unknown, None).is_err());
    }

    #[test]
    fn routed_and_legacy_unknown_records_are_not_recoverable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        append(&path, &sample_record()).unwrap();
        assert!(
            record_state(
                &path,
                "run-1",
                RunState::Interrupted,
                Outcome::Unknown,
                None
            )
            .is_err()
        );
        assert_eq!(load(&path).unwrap()[0].lifecycle, None);
    }

    #[test]
    fn appends_loads_and_updates_records() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("runs.jsonl");
        let record = sample_record();

        append(&path, &record).expect("record appends");
        assert_eq!(load(&path).expect("records load"), vec![record.clone()]);

        let updated =
            update_outcome(&path, "run-1", Outcome::Success, None).expect("outcome updates");
        assert_eq!(updated.outcome, Outcome::Success);
        assert_eq!(
            load(&path).expect("records reload")[0].outcome,
            Outcome::Success
        );
    }

    #[test]
    fn records_execution_evidence_with_the_outcome() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("runs.jsonl");
        append(&path, &sample_record()).expect("record appends");
        let execution = ExecutionEvidence {
            observations: None,
            harness: "agent".to_owned(),
            model: "provider/model".to_owned(),
            duration_ms: 42,
            exit_code: Some(0),
            verification: None,
        };

        let updated = record_execution(&path, "run-1", Outcome::Success, execution.clone())
            .expect("execution updates");

        assert_eq!(updated.outcome, Outcome::Success);
        assert_eq!(updated.execution, Some(execution));
        assert_eq!(
            updated.outcome_evidence.unwrap().source,
            OutcomeSource::ProcessExit
        );
    }

    #[test]
    fn feedback_requires_a_reason_for_reversal_and_preserves_audit_history() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("runs.jsonl");
        append(&path, &sample_record()).unwrap();
        let first = update_outcome(&path, "run-1", Outcome::Success, None).unwrap();
        assert!(first.is_learning_evidence());
        let before = fs::read(&path).unwrap();
        assert!(update_outcome(&path, "run-1", Outcome::Failure, Some("  ")).is_err());
        assert_eq!(before, fs::read(&path).unwrap());
        let changed =
            update_outcome(&path, "run-1", Outcome::Failure, Some(" tests fail ")).unwrap();
        assert_eq!(changed.schema_version, RECORD_SCHEMA_VERSION);
        assert_eq!(changed.feedback.len(), 2);
        assert_eq!(changed.feedback[0].previous_outcome, Outcome::Unknown);
        assert_eq!(changed.feedback[1].previous_outcome, Outcome::Success);
        assert_eq!(
            changed.feedback[1].previous_source,
            Some(OutcomeSource::Manual)
        );
        assert_eq!(changed.feedback[1].reason.as_deref(), Some("tests fail"));
        assert_eq!(load(&path).unwrap()[0], changed);
    }

    #[test]
    fn concurrent_appends_preserve_every_record() {
        const WRITERS: usize = 16;

        let directory = tempdir().expect("temporary directory");
        let path = Arc::new(directory.path().join("runs.jsonl"));
        let barrier = Arc::new(Barrier::new(WRITERS));
        let writers: Vec<_> = (0..WRITERS)
            .map(|index| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut record = sample_record();
                    record.decision.run_id = format!("run-{index}");
                    barrier.wait();
                    append(&path, &record).expect("concurrent append succeeds");
                })
            })
            .collect();

        for writer in writers {
            writer.join().expect("writer does not panic");
        }

        let records = load(&path).expect("records load");
        let run_ids: BTreeSet<_> = records
            .iter()
            .map(|record| record.decision.run_id.as_str())
            .collect();
        assert_eq!(records.len(), WRITERS);
        assert_eq!(run_ids.len(), WRITERS);
    }

    #[test]
    fn concurrent_append_and_update_do_not_lose_a_record() {
        let directory = tempdir().expect("temporary directory");
        let path = Arc::new(directory.path().join("runs.jsonl"));
        append(&path, &sample_record()).expect("initial record appends");
        let barrier = Arc::new(Barrier::new(2));

        let update_path = Arc::clone(&path);
        let update_barrier = Arc::clone(&barrier);
        let updater = thread::spawn(move || {
            update_barrier.wait();
            update_outcome(&update_path, "run-1", Outcome::Success, None)
                .expect("concurrent update succeeds");
        });

        let append_path = Arc::clone(&path);
        let append_barrier = Arc::clone(&barrier);
        let appender = thread::spawn(move || {
            let mut record = sample_record();
            record.decision.run_id = "run-2".to_owned();
            append_barrier.wait();
            append(&append_path, &record).expect("concurrent append succeeds");
        });

        updater.join().expect("updater does not panic");
        appender.join().expect("appender does not panic");

        let records = load(&path).expect("records load");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].outcome, Outcome::Success);
        assert_eq!(records[1].decision.run_id, "run-2");
    }

    #[test]
    fn malformed_history_is_reported_without_being_rewritten() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("runs.jsonl");
        let malformed = b"{not valid json}\n";
        fs::write(&path, malformed).expect("malformed history is written");

        let error = load(&path).expect_err("malformed history is rejected");

        assert!(error.to_string().contains("invalid run record on line 1"));
        assert_eq!(
            fs::read(&path).expect("history remains readable"),
            malformed
        );
    }

    #[test]
    fn external_completion_rejects_nonpending_states_without_mutation() {
        for state in [
            RunState::Running,
            RunState::Verifying,
            RunState::Completed,
            RunState::Interrupted,
            RunState::LaunchFailed,
            RunState::Cancelled,
            RunState::TimedOut,
        ] {
            let mut record = sample_record();
            record.lifecycle = Some(RunLifecycle {
                state,
                ..Default::default()
            });
            let before = record.clone();
            assert!(apply_external_completion(&mut record, Outcome::Success, None).is_err());
            assert_eq!(record, before);
        }
        let mut legacy = sample_record();
        apply_external_completion(&mut legacy, Outcome::Unknown, None).unwrap();
        assert_eq!(legacy.lifecycle.unwrap().state, RunState::Completed);
        assert!(legacy.execution.is_none());
    }

    fn sample_record() -> RouteRecord {
        RouteRecord {
            schema_version: 1,
            decision: RouteDecision {
                run_id: "run-1".to_owned(),
                tier: "balanced".to_owned(),
                suggested_tier: "balanced".to_owned(),
                confidence: 0.8,
                probabilities: BTreeMap::new(),
                fallback_applied: false,
                jev_model: "jev-test".to_owned(),
                created_at_ms: 1,
                source: jevia_core::DecisionSource::Live,
            },
            task: Some("test task".to_owned()),
            outcome: Outcome::Unknown,
            execution: None,
            lifecycle: None,
            outcome_evidence: None,
            feedback: Vec::new(),
        }
    }
}
