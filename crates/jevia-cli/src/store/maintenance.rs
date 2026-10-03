use std::{collections::HashSet, path::PathBuf};

use crate::recordings::PendingRecordings;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::*;

#[derive(Debug, Clone, Copy)]
pub enum Maintenance {
    Repair,
    Archive { keep: usize },
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub operation: &'static str,
    pub applied: bool,
    pub would_change: bool,
    pub retained_records: usize,
    pub archived_records: usize,
    pub truncated_tail_bytes: usize,
    pub added_final_newline: bool,
    pub backup: Option<PathBuf>,
    pub archive: Option<PathBuf>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Scan {
    records: usize,
    candidates: usize,
    truncated_tail_bytes: usize,
    has_valid_bytes: bool,
    ends_with_newline: bool,
    digest: [u8; 32],
}

/// Preview by default. The same stable lock covers inspection, durable backup,
/// archive creation, and the atomic replacement; every apply reads a fresh plan.
pub fn maintain(path: &Path, operation: Maintenance, apply: bool) -> Result<Report> {
    if matches!(operation, Maintenance::Archive { keep: 0 }) {
        bail!("archive --keep must be greater than zero");
    }
    let mut report = Report {
        operation: match operation {
            Maintenance::Repair => "repair",
            Maintenance::Archive { .. } => "archive",
        },
        applied: false,
        would_change: false,
        retained_records: 0,
        archived_records: 0,
        truncated_tail_bytes: 0,
        added_final_newline: false,
        backup: None,
        archive: None,
    };
    let parent = path
        .parent()
        .context("history path has no parent directory")?;
    if !parent.exists() {
        return Ok(report);
    }
    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    let pending = if matches!(operation, Maintenance::Archive { .. }) {
        Some(PendingRecordings::scan(parent)?)
    } else {
        None
    };
    let Some(mut file) = crate::regular_file::open_optional(path)
        .context("could not open history for maintenance")?
    else {
        return Ok(report);
    };
    // Preview keeps only identifiers for duplicate detection and one bounded line.
    // Validate everything before creating recovery files, even for a late error.
    let before = scan(&mut file, operation, pending.as_ref(), |_, _, _| Ok(()))?;
    report.archived_records = match operation {
        Maintenance::Repair => 0,
        Maintenance::Archive { keep } => before.candidates.saturating_sub(keep),
    };
    report.retained_records = before.records - report.archived_records;
    report.truncated_tail_bytes = before.truncated_tail_bytes;
    // keep >= 1 ensures the final eligible row is retained. Whitespace, pending
    // rows, and active rows are retained too, so the last valid byte survives.
    report.added_final_newline = (matches!(operation, Maintenance::Repair)
        || report.archived_records > 0)
        && before.has_valid_bytes
        && !before.ends_with_newline;
    report.would_change = report.archived_records > 0
        || report.truncated_tail_bytes > 0
        || report.added_final_newline;
    if !apply || !report.would_change {
        return Ok(report);
    }

    let mut backup = Snapshot::new(&parent.join("history-backups"))?;
    let mut archive = if report.archived_records > 0 {
        Some(Snapshot::new(&parent.join("history-archives"))?)
    } else {
        None
    };
    let mut replacement = NamedTempFile::new_in(parent)?;
    let mut remaining = report.archived_records;
    file.rewind()?;
    let after = {
        let mut retained = BufWriter::new(replacement.as_file_mut());
        let after = scan(
            &mut file,
            operation,
            pending.as_ref(),
            |bytes, eligible, truncated| {
                backup.writer.write_all(bytes)?;
                if truncated {
                    return Ok(());
                }
                if eligible && remaining > 0 {
                    archive
                        .as_mut()
                        .context("missing archive writer")?
                        .writer
                        .write_all(bytes)?;
                    remaining -= 1;
                } else {
                    retained.write_all(bytes)?;
                }
                Ok(())
            },
        )?;
        if report.added_final_newline {
            retained.write_all(b"\n")?;
        }
        retained.flush()?;
        after
    };
    validate_snapshot(&before, &after, remaining)?;
    drop(file); // Windows replacement cannot retain an open source handle.
    replacement.as_file().sync_all()?;
    // Publish durable, exact recovery bytes before replacing history. Includes
    // truncated tails and unknown additive fields; never normalize user records.
    let backup = backup.finish()?;
    report.backup = Some(backup.clone());
    if let Some(archive) = archive {
        report.archive = Some(archive.finish().with_context(|| {
            format!(
                "archive failed; original history backup: {}",
                backup.display()
            )
        })?);
    }
    if let Some(pending) = pending {
        pending.recheck(parent)?;
    }
    replacement
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "could not commit maintenance; original history backup: {}",
                backup.display()
            )
        })?;
    sync_parent(path)?;
    report.applied = true;
    Ok(report)
}

fn validate_snapshot(before: &Scan, after: &Scan, remaining: usize) -> Result<()> {
    if before != after || remaining != 0 {
        bail!("history changed during maintenance; original history was not replaced");
    }
    Ok(())
}

/// Two passes under one exclusive lock. Memory grows with unique IDs, not task
/// text, event payloads, or copies of the entire source/retained/archive streams.
fn scan(
    file: &mut File,
    operation: Maintenance,
    pending: Option<&PendingRecordings>,
    mut visit: impl FnMut(&[u8], bool, bool) -> Result<()>,
) -> Result<Scan> {
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    let mut ids = HashSet::new();
    let mut result = Scan::default();
    let mut hash = Sha256::new();
    let mut index = 0;
    while crate::jsonl::read_line(&mut reader, &mut bytes, || Ok(()))? {
        index += 1;
        hash.update(&bytes);
        let mut eligible = false;
        if !bytes.iter().all(u8::is_ascii_whitespace) {
            let value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(error)
                    if matches!(operation, Maintenance::Repair)
                        && error.is_eof()
                        && !bytes.ends_with(b"\n") =>
                {
                    result.truncated_tail_bytes = bytes.len();
                    visit(&bytes, false, true)?;
                    break;
                }
                Err(error) => {
                    return Err(crate::diagnostics::json_line("JSON", index, &error))
                        .context("refusing to rewrite history");
                }
            };
            let record: RouteRecord = serde_json::from_value(value)
                .map_err(|error| crate::diagnostics::json_line("record", index, &error))
                .context("refusing to rewrite history")?;
            if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
                bail!(
                    "unsupported schema {} on line {index}; refusing to rewrite history",
                    record.schema_version
                );
            }
            record.decision.validate().map_err(anyhow::Error::msg).with_context(||
                format!("invalid routing decision on line {index} (contents redacted); refusing to rewrite history"))?;
            eligible = archivable(&record)
                && pending.is_none_or(|pending| !pending.contains(&record.decision.run_id));
            if !ids.insert(record.decision.run_id) {
                bail!("duplicate run id on line {index}; refusing to rewrite history");
            }
            result.records += 1;
            result.candidates += usize::from(eligible);
        }
        result.has_valid_bytes = true;
        result.ends_with_newline = bytes.ends_with(b"\n");
        visit(&bytes, eligible, false)?;
    }
    result.digest = hash.finalize().into();
    Ok(result)
}

pub(crate) fn archivable(record: &RouteRecord) -> bool {
    match &record.lifecycle {
        Some(life) => !life.state.is_active() && life.state != RunState::Routed,
        None => record.outcome != Outcome::Unknown,
    }
}

struct Snapshot {
    writer: BufWriter<NamedTempFile>,
}

impl Snapshot {
    fn new(directory: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(directory)
            .context("could not create history snapshot directory")?;
        sync_parent(directory)?;
        let temporary = tempfile::Builder::new()
            .prefix("runs-")
            .suffix(".jsonl")
            .tempfile_in(directory)?;
        Ok(Self {
            writer: BufWriter::new(temporary),
        })
    }

    fn finish(mut self) -> Result<PathBuf> {
        self.writer.flush()?;
        self.writer.get_ref().as_file().sync_all()?;
        let temporary = self
            .writer
            .into_inner()
            .map_err(|error| error.into_error())?;
        let (_, path) = temporary.keep().map_err(|error| error.error)?;
        sync_parent(&path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn row(id: &str, state: Option<RunState>, outcome: Outcome) -> Vec<u8> {
        let value = serde_json::json!({
            "schema_version": 3, "run_id": id, "tier": "fast", "suggested_tier": "fast",
            "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
            "jev_model": "test", "created_at_ms": 1, "task": "private task", "outcome": outcome,
            "lifecycle": state.map(|state| jevia_core::RunLifecycle {
                state, started_at_ms: Some(1), finished_at_ms: None,
            }),
            "additional_metadata": {"preserve": true}
        });
        format!("{value}\n").into_bytes()
    }

    #[test]
    fn repair_is_preview_first_and_backs_up_exact_truncated_bytes() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let valid = row("first", None, Outcome::Success);
        let original = [valid.clone(), b"{\"schema_version\":".to_vec()].concat();
        fs::write(&path, &original).unwrap();
        let preview = maintain(&path, Maintenance::Repair, false).unwrap();
        assert!(!preview.applied);
        assert!(preview.would_change);
        assert_eq!(preview.retained_records, 1);
        assert_eq!(preview.truncated_tail_bytes, 18);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!root.path().join("history-backups").exists());
        let applied = maintain(&path, Maintenance::Repair, true).unwrap();
        assert!(applied.applied);
        assert_eq!(fs::read(applied.backup.unwrap()).unwrap(), original);
        assert_eq!(fs::read(&path).unwrap(), valid);
        assert_eq!(load(&path).unwrap().len(), 1);
        let unchanged = maintain(&path, Maintenance::Repair, true).unwrap();
        assert!(!unchanged.would_change);
        assert!(unchanged.backup.is_none());
    }

    #[test]
    fn repair_adds_missing_final_newline_without_changing_record_bytes() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let mut bytes = row("first", None, Outcome::Success);
        bytes.pop();
        fs::write(&path, &bytes).unwrap();
        let report = maintain(&path, Maintenance::Repair, true).unwrap();
        assert!(report.added_final_newline);
        assert_eq!(report.truncated_tail_bytes, 0);
        assert_eq!(fs::read(report.backup.unwrap()).unwrap(), bytes);
        bytes.push(b'\n');
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn ambiguous_or_unsupported_history_is_never_rewritten() {
        let valid = row("first", None, Outcome::Success);
        let future = String::from_utf8(valid.clone())
            .unwrap()
            .replace("\"schema_version\":3", "\"schema_version\":99")
            .into_bytes();
        for bytes in [
            [b"{broken\n".to_vec(), valid.clone()].concat(),
            [valid.clone(), b"{\"schema_version\":\n".to_vec()].concat(),
            [valid.clone(), b"{\"schema_version\":oops}".to_vec()].concat(),
            [valid.clone(), b"{}".to_vec()].concat(),
            [valid.clone(), valid.clone()].concat(),
            future,
        ] {
            let root = tempdir().unwrap();
            let path = root.path().join("runs.jsonl");
            fs::write(&path, &bytes).unwrap();
            for operation in [Maintenance::Repair, Maintenance::Archive { keep: 1 }] {
                assert!(maintain(&path, operation, true).is_err());
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert!(!root.path().join("history-backups").exists());
            }
        }
    }

    #[test]
    fn archive_preserves_active_pending_and_ambiguous_legacy_records() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let old = row("old", Some(RunState::Completed), Outcome::Success);
        let running = row("running", Some(RunState::Running), Outcome::Unknown);
        let pending = row("pending", Some(RunState::Routed), Outcome::Unknown);
        let legacy = row("legacy", None, Outcome::Unknown);
        let latest = row("latest", Some(RunState::Interrupted), Outcome::Unknown);
        let original = [
            old.clone(),
            running.clone(),
            pending.clone(),
            legacy.clone(),
            latest.clone(),
        ]
        .concat();
        fs::write(&path, &original).unwrap();
        let operation = Maintenance::Archive { keep: 1 };
        let preview = maintain(&path, operation, false).unwrap();
        assert_eq!(preview.archived_records, 1);
        assert_eq!(preview.retained_records, 4);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!root.path().join("history-archives").exists());
        let applied = maintain(&path, operation, true).unwrap();
        assert_eq!(fs::read(applied.backup.unwrap()).unwrap(), original);
        assert_eq!(fs::read(applied.archive.unwrap()).unwrap(), old);
        assert_eq!(
            fs::read(&path).unwrap(),
            [running, pending, legacy, latest].concat()
        );
        assert!(!maintain(&path, operation, true).unwrap().would_change);
    }

    #[test]
    fn snapshot_failure_leaves_original_history_untouched() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let original = [row("first", None, Outcome::Success), b"{".to_vec()].concat();
        fs::write(&path, &original).unwrap();
        fs::write(root.path().join("history-backups"), b"not a directory").unwrap();
        assert!(maintain(&path, Maintenance::Repair, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn archive_is_replanned_on_apply_and_zero_retention_is_rejected() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let first = row("first", None, Outcome::Success);
        let second = row("second", None, Outcome::Success);
        let third = row("third", None, Outcome::Success);
        fs::write(&path, [first.clone(), second.clone()].concat()).unwrap();
        assert!(maintain(&path, Maintenance::Archive { keep: 0 }, true).is_err());
        let operation = Maintenance::Archive { keep: 1 };
        assert_eq!(
            maintain(&path, operation, false).unwrap().archived_records,
            1
        );
        fs::write(&path, [first, second, third.clone()].concat()).unwrap();
        assert_eq!(
            maintain(&path, operation, true).unwrap().archived_records,
            2
        );
        assert_eq!(fs::read(&path).unwrap(), third);
    }

    #[test]
    fn streaming_archive_preserves_raw_bytes_across_buffers_and_trailing_whitespace() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let mut original = b" \r\n".to_vec();
        let mut expected_archive = Vec::new();
        for id in 0..500 {
            let mut raw = row(
                &format!("run-{id}"),
                Some(RunState::Completed),
                Outcome::Unknown,
            );
            raw.pop();
            raw.extend_from_slice(b"\r\n");
            original.extend_from_slice(&raw);
            if id < 498 {
                expected_archive.extend_from_slice(&raw);
            }
        }
        // Keep unknown JSON fields, CRLF, blank lines and an unterminated space.
        original.extend_from_slice(b"\r\n ");
        fs::write(&path, &original).unwrap();
        let operation = Maintenance::Archive { keep: 2 };
        let preview = maintain(&path, operation, false).unwrap();
        assert_eq!(preview.archived_records, 498);
        assert_eq!(preview.retained_records, 2);
        assert!(preview.added_final_newline);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        let applied = maintain(&path, operation, true).unwrap();
        assert_eq!(fs::read(applied.backup.unwrap()).unwrap(), original);
        assert_eq!(
            fs::read(applied.archive.unwrap()).unwrap(),
            expected_archive
        );
        let expected = [
            b" \r\n".as_slice(),
            &original[3 + expected_archive.len()..],
            b"\n",
        ]
        .concat();
        assert_eq!(fs::read(&path).unwrap(), expected);
    }

    #[test]
    fn two_pass_validation_detects_changed_bytes_even_with_the_same_counts() {
        let root = tempdir().unwrap();
        let path = root.path().join("runs.jsonl");
        let first = row("first", None, Outcome::Success);
        fs::write(&path, &first).unwrap();
        let scan_file = || {
            scan(
                &mut File::open(&path).unwrap(),
                Maintenance::Repair,
                None,
                |_, _, _| Ok(()),
            )
            .unwrap()
        };
        let before = scan_file();
        validate_snapshot(&before, &scan_file(), 0).unwrap();
        fs::write(
            &path,
            String::from_utf8(first)
                .unwrap()
                .replace("private task", "changed task"),
        )
        .unwrap();
        let after = scan_file();
        assert_eq!(before.records, after.records);
        assert_eq!(before.candidates, after.candidates);
        assert!(validate_snapshot(&before, &after, 0).is_err());
        assert!(validate_snapshot(&before, &before, 1).is_err());
    }

    #[test]
    fn streaming_repair_handles_empty_and_whitespace_only_inputs() {
        for raw in [b"".as_slice(), b" \r\n", b" ", b"{"] {
            let root = tempdir().unwrap();
            let path = root.path().join("runs.jsonl");
            fs::write(&path, raw).unwrap();
            let applied = maintain(&path, Maintenance::Repair, true).unwrap();
            assert_eq!(applied.retained_records, 0);
            let expected = match raw {
                b"{" => b"".as_slice(),
                b" " => b" \n",
                _ => raw,
            };
            assert_eq!(fs::read(&path).unwrap(), expected);
            assert_eq!(applied.would_change, raw != expected);
            if let Some(backup) = applied.backup {
                assert_eq!(fs::read(backup).unwrap(), raw);
            }
        }
    }
}
