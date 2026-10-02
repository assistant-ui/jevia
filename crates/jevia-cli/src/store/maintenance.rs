use std::{collections::HashSet, path::PathBuf};

use serde::Serialize;

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

struct Line<'a> {
    bytes: &'a [u8],
    record: Option<RouteRecord>,
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
    let original = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(error).context("could not read history for maintenance"),
    };
    let mut lines = Vec::new();
    let mut ids = HashSet::new();
    let mut offset = 0;
    for (index, bytes) in original.split_inclusive(|byte| *byte == b'\n').enumerate() {
        // Match normal JSONL reads, including room for a repaired final newline.
        // Even whitespace and truncated tails must not bypass the input bound.
        if bytes.len() > crate::jsonl::MAX_LINE_BYTES
            || (bytes.len() == crate::jsonl::MAX_LINE_BYTES && !bytes.ends_with(b"\n"))
        {
            bail!(
                "JSONL line {} exceeds the 8 MiB limit including its newline (contents redacted); refusing to rewrite history",
                index + 1
            );
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            lines.push(Line {
                bytes,
                record: None,
            });
            offset += bytes.len();
            continue;
        }
        // Distinguish syntactically truncated JSON from a complete JSON value
        // whose schema/types we do not understand. Never repair the latter.
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(error)
                if matches!(operation, Maintenance::Repair)
                    && error.is_eof()
                    && !bytes.ends_with(b"\n")
                    && offset + bytes.len() == original.len() =>
            {
                report.truncated_tail_bytes = bytes.len();
                break;
            }
            Err(error) => {
                return Err(crate::diagnostics::json_line("JSON", index + 1, &error))
                    .context("refusing to rewrite history");
            }
        };
        let record: RouteRecord = serde_json::from_value(value)
            .map_err(|error| crate::diagnostics::json_line("record", index + 1, &error))
            .context("refusing to rewrite history")?;
        if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
            bail!(
                "unsupported schema {} on line {}; refusing to rewrite history",
                record.schema_version,
                index + 1
            );
        }
        record
            .decision
            .validate()
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "invalid routing decision on line {} (contents redacted); refusing to rewrite history",
                    index + 1
                )
            })?;
        if !ids.insert(record.decision.run_id.clone()) {
            bail!(
                "duplicate run id on line {}; refusing to rewrite history",
                index + 1
            );
        }
        lines.push(Line {
            bytes,
            record: Some(record),
        });
        offset += bytes.len();
    }
    let candidates = lines
        .iter()
        .filter(|line| line.record.as_ref().is_some_and(archivable))
        .count();
    let mut to_archive = match operation {
        Maintenance::Repair => 0,
        Maintenance::Archive { keep } => candidates.saturating_sub(keep),
    };
    let mut retained = Vec::new();
    let mut archived = Vec::new();
    for line in lines {
        if to_archive > 0 && line.record.as_ref().is_some_and(archivable) {
            archived.extend_from_slice(line.bytes);
            report.archived_records += 1;
            to_archive -= 1;
        } else {
            retained.extend_from_slice(line.bytes);
            report.retained_records += usize::from(line.record.is_some());
        }
    }
    if (matches!(operation, Maintenance::Repair) || report.archived_records > 0)
        && !retained.is_empty()
        && !retained.ends_with(b"\n")
    {
        retained.push(b'\n');
        report.added_final_newline = true;
    }
    report.would_change = retained != original;
    if !apply || !report.would_change {
        return Ok(report);
    }

    // Save exact bytes, including corruption and unknown additive JSON fields,
    // before changing anything. No snapshot is ever overwritten or auto-deleted.
    let backup = snapshot(&parent.join("history-backups"), &original)?;
    report.backup = Some(backup.clone());
    if !archived.is_empty() {
        report.archive = Some(
            snapshot(&parent.join("history-archives"), &archived).with_context(|| {
                format!(
                    "archive failed; original history backup: {}",
                    backup.display()
                )
            })?,
        );
    }
    replace(path, &retained).with_context(|| {
        format!(
            "could not commit maintenance; original history backup: {}",
            backup.display()
        )
    })?;
    report.applied = true;
    Ok(report)
}

pub(crate) fn archivable(record: &RouteRecord) -> bool {
    match &record.lifecycle {
        Some(life) => !life.state.is_active() && life.state != RunState::Routed,
        None => record.outcome != Outcome::Unknown,
    }
}

fn snapshot(directory: &Path, bytes: &[u8]) -> Result<PathBuf> {
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
    let mut temporary = tempfile::Builder::new()
        .prefix("runs-")
        .suffix(".jsonl")
        .tempfile_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    let (_, path) = temporary.keep().map_err(|error| error.error)?;
    sync_parent(&path)?;
    Ok(path)
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary =
        NamedTempFile::new_in(path.parent().context("history path has no parent")?)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(path)
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
}
