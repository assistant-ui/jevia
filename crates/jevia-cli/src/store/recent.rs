//! Aggregate the recent window without retaining its full record payloads.

use super::*;

/// Visit the latest `limit` records in append order, returning whether older
/// records exist. Both passes share a file handle and lock, so a cooperating
/// writer cannot change the window between counting and aggregation.
pub fn visit_recent(
    path: &Path,
    limit: usize,
    mut visit: impl FnMut(&RouteRecord) -> Result<()>,
) -> Result<bool> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok(false);
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let Some(file) = crate::regular_file::open_optional(path)? else {
        return Ok(false);
    };
    let mut reader = BufReader::new(file);
    let mut count = 0usize;
    let mut line = Vec::new();
    // Cheap framing pass: do not deserialize (or retain) task/event payloads.
    // Match the parser's treatment of blank lines, including Unicode whitespace.
    while read_line_until(&mut reader, &mut line, None)? {
        let text = std::str::from_utf8(&line)
            .context("could not read history (invalid UTF-8; contents redacted)")?;
        count += usize::from(!text.trim().is_empty());
    }
    drop(line);
    reader.rewind().context("could not rewind run history")?;
    let mut skip = count.saturating_sub(limit);
    // Preserve validation of the ENTIRE history, even outside the stats window.
    // Deserialize each record only once and release it after the callback.
    read_records_from(reader, path, |record| {
        if skip > 0 {
            skip -= 1;
        } else {
            visit(&record)?;
        }
        Ok(())
    })?;
    Ok(count > limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_missing_and_whitespace_history_have_no_older_records() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            dir.path().join("missing/runs.jsonl"),
            dir.path().join("runs.jsonl"),
        ] {
            assert!(!visit_recent(&path, 10, |_| panic!("empty history")).unwrap());
        }
        let path = dir.path().join("runs.jsonl");
        for contents in ["", "\n\r\n \t\n\u{2003}"] {
            fs::write(&path, contents).unwrap();
            assert!(!visit_recent(&path, 0, |_| panic!("empty history")).unwrap());
        }
    }

    #[test]
    fn framing_matches_parser_and_keeps_append_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let mut records = Vec::new();
        let mut raw = "\n\u{2003}\n".to_string();
        for index in 0..7 {
            let mut record = crate::tests::sample_record();
            record.decision.run_id = format!("run-{index}");
            record.decision.created_at_ms = 20 - index;
            raw.push_str(&serde_json::to_string(&record).unwrap());
            if index != 6 {
                raw.push_str("\r\n \t\n");
            }
            records.push(record);
        }
        fs::write(&path, raw).unwrap();
        for limit in [0, 1, 3, 7, 8, 100_000] {
            let mut actual = Vec::new();
            let older = visit_recent(&path, limit, |record| {
                actual.push(record.clone());
                Ok(())
            })
            .unwrap();
            assert_eq!(actual, records[records.len().saturating_sub(limit)..]);
            assert_eq!(older, records.len() > limit);
        }
    }

    #[test]
    fn invalid_old_records_are_still_rejected_with_redacted_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let valid = serde_json::to_string(&crate::tests::sample_record()).unwrap();
        let mut unsupported = crate::tests::sample_record();
        unsupported.schema_version = 999;
        let mut invalid_decision = crate::tests::sample_record();
        invalid_decision.decision.confidence = 2.0;
        for invalid in [
            b"{PRIVATE malformed}".to_vec(),
            b"PRIVATE\xff".to_vec(),
            serde_json::to_vec(&unsupported).unwrap(),
            serde_json::to_vec(&invalid_decision).unwrap(),
            vec![b' '; crate::jsonl::MAX_LINE_BYTES + 1],
        ] {
            let mut raw = invalid;
            raw.extend_from_slice(format!("\n{valid}\n{valid}\n").as_bytes());
            fs::write(&path, &raw).unwrap();
            let error = visit_recent(&path, 1, |_| Ok(())).unwrap_err();
            assert!(!format!("{error:#}").contains("PRIVATE"));
            assert_eq!(fs::read(&path).unwrap(), raw);
        }
    }

    #[test]
    fn callback_failure_releases_snapshot_and_never_returns_partial_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let record = crate::tests::sample_record();
        append(&path, &record).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(visit_recent(&path, 1, |_| bail!("callback failed")).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        // Exclusive acquisition succeeds after the failed shared reader returns.
        assert!(acquire_lock(&path, LockMode::Exclusive).is_ok());
    }
}
