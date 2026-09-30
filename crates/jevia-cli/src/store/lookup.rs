use super::*;

/// Keep only the requested record, but validate the entire shared-lock snapshot.
pub fn get(path: &Path, id: &str) -> Result<RouteRecord> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        bail!("run id was not found in history");
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    get_unlocked(path, id)
}

/// Background replay never waits for a busy writer or loads unrelated records.
pub fn try_get(path: &Path, id: &str) -> Result<RouteRecord> {
    let lock = private_lock_options().open(path.with_extension("lock"))?;
    lock.try_lock_shared()
        .context("history busy; replay deferred")?;
    let _guard = crate::lease::FileLock::new(lock);
    get_unlocked(path, id)
}

fn get_unlocked(path: &Path, id: &str) -> Result<RouteRecord> {
    let mut found = None;
    read_records(path, |record| {
        // Retain the old first-match behavior for duplicate identities. The
        // explicit deep check diagnoses duplicates; lookup never rewrites them.
        if found.is_none() && record.decision.run_id == id {
            found = Some(record);
        }
        Ok(())
    })?;
    found.context("run id was not found in history")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: usize) -> RouteRecord {
        serde_json::from_value(json!({
            "schema_version": 6, "run_id": format!("run-{id}"), "tier": "fast", "suggested_tier": "fast",
            "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
            "created_at_ms": id, "task": "private 🦀", "outcome": "unknown"
        })).unwrap()
    }

    #[test]
    fn streaming_lookup_preserves_first_match_missing_ids_and_raw_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        assert!(get(&path, "missing").is_err());
        let missing = dir.path().join("not-created/runs.jsonl");
        assert!(get(&missing, "missing").is_err());
        assert!(!missing.parent().unwrap().exists());
        let mut raw = String::new();
        for id in 0..1000 {
            raw.push_str(&format!(
                "{}\r\n",
                serde_json::to_string(&record(id)).unwrap()
            ));
        }
        let mut duplicate = record(0);
        duplicate.task = Some("later duplicate".into());
        raw.push_str(&serde_json::to_string(&duplicate).unwrap()); // No final newline.
        fs::write(&path, &raw).unwrap();
        for id in [0, 500, 999] {
            assert_eq!(get(&path, &format!("run-{id}")).unwrap(), record(id));
            assert_eq!(try_get(&path, &format!("run-{id}")).unwrap(), record(id));
        }
        assert!(get(&path, "missing").is_err());
        assert!(try_get(&path, "missing").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }

    #[test]
    fn lookup_never_returns_a_match_before_validating_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let valid = serde_json::to_string(&record(0)).unwrap();
        let mut unsupported = record(1);
        unsupported.schema_version = 999;
        for invalid in [
            "{private-invalid".into(),
            serde_json::to_string(&unsupported).unwrap(),
        ] {
            for raw in [
                format!("{valid}\n{invalid}\n"),
                format!("{invalid}\n{valid}\n"),
            ] {
                fs::write(&path, &raw).unwrap();
                for lookup in [get, try_get] {
                    let error = lookup(&path, "run-0").unwrap_err();
                    assert!(!format!("{error:#}").contains("private-invalid"));
                }
                assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            }
        }
    }

    #[test]
    fn replay_lookup_skips_a_busy_history_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        append(&path, &record(0)).unwrap();
        let held = acquire_lock(&path, LockMode::Exclusive).unwrap();
        let started = std::time::Instant::now();
        let error = try_get(&path, "run-0").unwrap_err();
        assert!(format!("{error:#}").contains("history busy"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        drop(held);
        assert_eq!(try_get(&path, "run-0").unwrap(), record(0));
    }
}
