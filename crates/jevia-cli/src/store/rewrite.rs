use super::*;

/// Caller holds the exclusive history lock through validation and publication.
/// Memory scales with the largest record, not the number of retained runs.
pub(super) fn update_unlocked(
    path: &Path,
    run_id: &str,
    update_record: impl FnOnce(&mut RouteRecord) -> Result<()>,
) -> Result<RouteRecord> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    let mut temporary =
        NamedTempFile::new_in(parent).context("could not create temporary history")?;
    let result = {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let result = stream_update(path, run_id, update_record, &mut writer)?;
        writer
            .flush()
            .context("could not flush updated run history")?;
        result
    };
    temporary
        .as_file()
        .sync_all()
        .context("could not sync updated run history")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("could not atomically replace run history")?;
    sync_parent(path)?;
    Ok(result)
}

fn stream_update(
    path: &Path,
    run_id: &str,
    update_record: impl FnOnce(&mut RouteRecord) -> Result<()>,
    mut output: impl Write,
) -> Result<RouteRecord> {
    let mut update_record = Some(update_record);
    let mut result = None;
    read_records(path, |mut record| {
        // Preserve first-match semantics for legacy duplicate identities.
        if record.decision.run_id == run_id
            && let Some(update) = update_record.take()
        {
            update(&mut record)?;
            record.schema_version = RECORD_SCHEMA_VERSION;
            result = Some(record.clone());
        }
        serde_json::to_writer(&mut output, &record).context("could not encode run record")?;
        output
            .write_all(b"\n")
            .context("could not terminate run record")?;
        Ok(())
    })?;
    // A matching prefix never permits publishing a corrupt/unsupported suffix.
    result.context("run id was not found in local history")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: usize) -> RouteRecord {
        let mut record = crate::tests::sample_record();
        record.decision.run_id = format!("run-{id}");
        record.task = Some("fixture 🦀".into());
        record
    }

    #[test]
    fn streaming_updates_preserve_order_and_first_duplicate_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let rows: Vec<_> = (0..1000).map(record).chain([record(0)]).collect();
        let raw = rows
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n\n");
        for target in [0, 500, 999] {
            fs::write(&path, &raw).unwrap();
            let mut expected = rows.clone();
            expected[target].task = Some("changed".into());
            expected[target].schema_version = RECORD_SCHEMA_VERSION;
            let updated = update(&path, &format!("run-{target}"), |r| {
                r.task = Some("changed".into());
                Ok(())
            })
            .unwrap();
            assert_eq!(updated, expected[target]);
            assert_eq!(load(&path).unwrap(), expected);
            assert_eq!(count(&path).unwrap(), rows.len());
        }
    }

    #[test]
    fn late_invalid_input_and_rejected_updates_leave_original_bytes_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        assert_eq!(count(&dir.path().join("missing/runs.jsonl")).unwrap(), 0);
        let valid = serde_json::to_string(&record(0)).unwrap();
        let mut future = record(1);
        future.schema_version = 999;
        for suffix in [
            "{PRIVATE".to_owned(),
            serde_json::to_string(&future).unwrap(),
        ] {
            let raw = format!("{valid}\n{suffix}");
            fs::write(&path, &raw).unwrap();
            let error = update_outcome(&path, "run-0", Outcome::Success, None).unwrap_err();
            assert!(!format!("{error:#}").contains("PRIVATE"));
            assert!(count(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
        fs::write(&path, &valid).unwrap();
        assert!(update_outcome(&path, "missing", Outcome::Success, None).is_err());
        assert!(update(&path, "run-0", |_| bail!("rejected")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), valid);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            2,
            "no temporary files left behind"
        );
    }

    #[test]
    fn writer_failure_never_changes_source() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let raw = serde_json::to_string(&record(0)).unwrap();
        fs::write(&path, &raw).unwrap();
        assert!(stream_update(&path, "run-0", |_| Ok(()), Broken).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }
}
