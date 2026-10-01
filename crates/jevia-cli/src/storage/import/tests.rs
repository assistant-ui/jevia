use super::*;
use jevia_core::RunState;
use serde_json::json;
use std::fs;

fn record(index: usize) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 1 + index % 3, "run_id": format!("run-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 405_usize.saturating_sub(index), "task": "private-task".repeat(4096), "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()
}

#[test]
fn snapshot_streams_large_records_and_does_not_reopen_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.jsonl");
    let mut writer = BufWriter::new(File::create(&source).unwrap());
    for index in 0..405 {
        // Blank lines, CRLF and an unterminated final record remain supported.
        if index > 0 {
            writer.write_all(b"\r\n \r\n").unwrap();
        }
        serde_json::to_writer(&mut writer, &record(index)).unwrap();
    }
    writer.flush().unwrap();
    drop(writer);
    store::with_import_reader(&source, |_| {
        let writer_lock = File::options()
            .read(true)
            .write(true)
            .open(source.with_extension("lock"))?;
        assert!(writer_lock.try_lock().is_err());
        Ok(())
    })
    .unwrap();
    let snapshot = Snapshot::capture(&source).unwrap();
    assert_eq!(snapshot.count, 405);
    assert_eq!(
        Some(snapshot.fingerprint),
        source_fingerprint(&source).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = snapshot.file.metadata().unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 0); // No pathname can replace the captured file.
    }
    // The source lock is released once capture is complete, before SQL begins.
    let lock = File::options()
        .read(true)
        .write(true)
        .open(source.with_extension("lock"))
        .unwrap();
    lock.try_lock().unwrap();
    fs::write(&source, "private-replacement-not-json").unwrap();
    let mut records = snapshot.records();
    for index in 0..405 {
        assert_eq!(records.next().unwrap().unwrap(), record(index));
    }
    assert!(records.next().is_none());
}

#[test]
fn setup_snapshot_is_read_only_and_fingerprints_raw_source_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("runs.jsonl");
    assert!(Snapshot::capture_optional(&source).unwrap().is_none());
    assert_eq!(source_fingerprint(&source).unwrap(), None);
    fs::write(&source, b"").unwrap();
    let empty = Snapshot::capture_optional(&source).unwrap().unwrap();
    assert_eq!(empty.count, 0);
    assert_eq!(empty.fingerprint, <[u8; 32]>::from(Sha256::digest(b"")));
    let raw = format!(" \r\n{}\r\n", serde_json::to_string(&record(0)).unwrap());
    fs::write(&source, &raw).unwrap();
    let snapshot = Snapshot::capture_optional(&source).unwrap().unwrap();
    assert_eq!(snapshot.count, 1);
    assert_eq!(
        snapshot.fingerprint,
        <[u8; 32]>::from(Sha256::digest(raw.as_bytes()))
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    fs::write(&source, raw.replace(" \r\n", "\t\r\n")).unwrap();
    assert_ne!(
        Some(snapshot.fingerprint),
        source_fingerprint(&source).unwrap()
    );
    assert_eq!(snapshot.records().next().unwrap().unwrap(), record(0));
    fs::write(&source, "private-invalid").unwrap();
    let error = Snapshot::capture_optional(&source).err().unwrap();
    assert!(!format!("{error:#}").contains("private-invalid"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(Snapshot::capture_optional(dir.path()).is_err());
}

#[test]
fn snapshot_rejects_invalid_late_records_and_redacts_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.jsonl");
    let first = record(0);
    let mut active = record(1);
    active.lifecycle.as_mut().unwrap().state = RunState::Running;
    let mut verifying = active.clone();
    verifying.lifecycle.as_mut().unwrap().state = RunState::Verifying;
    let mut empty = record(1);
    empty.decision.run_id.clear();
    let mut future = record(1);
    future.schema_version = RECORD_SCHEMA_VERSION + 1;
    let mut zero = record(1);
    zero.schema_version = 0;
    for invalid in [first.clone(), active, verifying, empty, future, zero] {
        let input = format!(
            "{}\n{}",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&invalid).unwrap()
        );
        fs::write(&source, &input).unwrap();
        let error = Snapshot::capture(&source).err().unwrap();
        assert!(!format!("{error:#}").contains("private-"));
        assert!(validate_import(&[first.clone(), invalid]).is_err());
        assert_eq!(fs::read_to_string(&source).unwrap(), input);
    }
    let mut invalid_enum = serde_json::to_value(record(1)).unwrap();
    invalid_enum["outcome"] = json!("private-invalid-enum");
    for tail in ["{private-malformed".to_owned(), invalid_enum.to_string()] {
        fs::write(
            &source,
            format!("\n{}\n{tail}", serde_json::to_string(&first).unwrap()),
        )
        .unwrap();
        let error = Snapshot::capture(&source).err().unwrap();
        assert!(format!("{error:#}").contains("line 3"));
        assert!(!format!("{error:#}").contains("private-"));
    }
    fs::write(&source, b"\xffprivate-invalid-utf8").unwrap();
    let error = Snapshot::capture(&source).err().unwrap();
    assert!(!format!("{error:#}").contains("private-"));
}

#[test]
fn snapshot_distinguishes_empty_from_missing_or_non_file_sources() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("missing.jsonl");
    assert!(Snapshot::capture(&source).is_err());
    assert!(!source.with_extension("lock").exists());
    assert!(Snapshot::capture(dir.path()).is_err());
    fs::write(&source, "\n \r\n").unwrap();
    assert_eq!(Snapshot::capture(&source).unwrap().records().count(), 0);
}
