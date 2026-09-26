use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use jevia_core::{ExecutionEvidence, Outcome, RouteRecord};
use tempfile::NamedTempFile;

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
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not open run history at {}", path.display()));
        }
    };

    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!("could not read line {} from {}", index + 1, path.display())
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: RouteRecord = serde_json::from_str(&line).with_context(|| {
            format!(
                "invalid run record on line {} of {}",
                index + 1,
                path.display()
            )
        })?;
        if record.schema_version != 1 {
            bail!(
                "unsupported run record schema {} on line {} of {}",
                record.schema_version,
                index + 1,
                path.display()
            );
        }
        records.push(record);
    }
    Ok(records)
}

pub fn append(path: &Path, record: &RouteRecord) -> Result<()> {
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
    file.write_all(&encoded)
        .context("could not append run record")?;
    file.sync_data().context("could not sync run history")?;
    sync_parent(path)?;
    Ok(())
}

pub fn update_outcome(path: &Path, run_id: &str, outcome: Outcome) -> Result<RouteRecord> {
    update(path, run_id, |record| record.outcome = outcome)
}

pub fn record_execution(
    path: &Path,
    run_id: &str,
    outcome: Outcome,
    execution: ExecutionEvidence,
) -> Result<RouteRecord> {
    update(path, run_id, |record| {
        record.outcome = outcome;
        record.execution = Some(execution);
    })
}

fn update(
    path: &Path,
    run_id: &str,
    update_record: impl FnOnce(&mut RouteRecord),
) -> Result<RouteRecord> {
    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    let mut records = load_unlocked(path)?;
    let updated = records
        .iter_mut()
        .find(|record| record.decision.run_id == run_id)
        .context("run id was not found in local history")?;
    update_record(updated);
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

fn acquire_lock(path: &Path, mode: LockMode) -> Result<File> {
    let lock_path = path.with_extension("lock");
    let lock = private_lock_options()
        .open(&lock_path)
        .with_context(|| format!("could not open history lock at {}", lock_path.display()))?;
    match mode {
        LockMode::Shared => lock.lock_shared(),
        LockMode::Exclusive => lock.lock(),
    }
    .with_context(|| format!("could not acquire history lock at {}", lock_path.display()))?;
    Ok(lock)
}

fn private_append_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).append(true);

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
    fn appends_loads_and_updates_records() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("runs.jsonl");
        let record = sample_record();

        append(&path, &record).expect("record appends");
        assert_eq!(load(&path).expect("records load"), vec![record.clone()]);

        let updated = update_outcome(&path, "run-1", Outcome::Success).expect("outcome updates");
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
            harness: "agent".to_owned(),
            model: "provider/model".to_owned(),
            duration_ms: 42,
            exit_code: Some(0),
        };

        let updated = record_execution(&path, "run-1", Outcome::Success, execution.clone())
            .expect("execution updates");

        assert_eq!(updated.outcome, Outcome::Success);
        assert_eq!(updated.execution, Some(execution));
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
            update_outcome(&update_path, "run-1", Outcome::Success)
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
            },
            task: Some("test task".to_owned()),
            outcome: Outcome::Unknown,
            execution: None,
        }
    }
}
