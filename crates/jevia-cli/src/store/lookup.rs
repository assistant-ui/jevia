use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub const LOOKUP_BATCH_SIZE: usize = 32;

/// One fully validated snapshot for a bounded group of cleanup owners. Missing
/// IDs stay absent; duplicate IDs retain the existing first-match semantics.
/// Project each match during the scan, so unrelated task/evidence payloads do
/// not remain allocated for every owner in the batch. The whole history is
/// still validated under one shared lock before returning any result.
pub fn try_get_many<T>(
    path: &Path,
    ids: &BTreeSet<String>,
    mut project: impl FnMut(RouteRecord) -> T,
) -> Result<BTreeMap<String, T>> {
    if ids.len() > LOOKUP_BATCH_SIZE {
        bail!("history lookup batch is too large");
    }
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let lock = private_lock_options().open(path.with_extension("lock"))?;
    lock.try_lock_shared()
        .context("history busy; cleanup deferred")?;
    let _guard = crate::lease::FileLock::new(lock);
    let mut found = BTreeMap::new();
    read_records(path, |record| {
        if ids.contains(&record.decision.run_id) {
            found
                .entry(record.decision.run_id.clone())
                .or_insert_with(|| project(record));
        }
        Ok(())
    })?;
    Ok(found)
}

/// Keep only the requested record, but validate the entire shared-lock snapshot.
pub fn get(path: &Path, id: &str) -> Result<RouteRecord> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        bail!("run id was not found in history");
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    get_unlocked(path, id, None)
}

/// Background replay never waits for a busy writer or loads unrelated records.
pub fn try_get(path: &Path, id: &str) -> Result<RouteRecord> {
    try_get_until(path, id, None)
}

pub fn try_get_until(
    path: &Path,
    id: &str,
    deadline: Option<std::time::Instant>,
) -> Result<RouteRecord> {
    check_deadline(deadline)?;
    let lock = private_lock_options().open(path.with_extension("lock"))?;
    lock.try_lock_shared()
        .context("history busy; replay deferred")?;
    let _guard = crate::lease::FileLock::new(lock);
    get_unlocked(path, id, deadline)
}

fn get_unlocked(
    path: &Path,
    id: &str,
    deadline: Option<std::time::Instant>,
) -> Result<RouteRecord> {
    let mut found = None;
    read_records_until(path, deadline, |record| {
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
    fn batch_lookup_is_bounded_validates_the_tail_and_keeps_first_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let first = record(0);
        let mut duplicate = first.clone();
        duplicate.task = Some("later".into());
        let raw = [first.clone(), record(1), duplicate]
            .iter()
            .map(|r| serde_json::to_string(r).unwrap() + "\n")
            .collect::<String>();
        fs::write(&path, &raw).unwrap();
        let ids = ["run-0".to_owned(), "missing".to_owned()]
            .into_iter()
            .collect();
        let found = try_get_many(&path, &ids, |record| record).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found["run-0"], first);
        let oversized = (0..=LOOKUP_BATCH_SIZE)
            .map(|i| format!("run-{i}"))
            .collect();
        assert!(try_get_many(&path, &oversized, |record| record).is_err());
        let held = acquire_lock(&path, LockMode::Exclusive).unwrap();
        assert!(try_get_many(&path, &ids, |record| record).is_err());
        drop(held);
        fs::write(&path, format!("{raw}PRIVATE invalid tail")).unwrap();
        let error = try_get_many(&path, &ids, |record| record).unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE"));
    }

    #[test]
    fn batch_projects_only_the_first_requested_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let mut raw = String::new();
        for id in [0, 1, 0, 2] {
            let mut record = record(id);
            record.task = Some("x".repeat(1024 * 1024));
            raw.push_str(&serde_json::to_string(&record).unwrap());
            raw.push('\n');
        }
        fs::write(&path, &raw).unwrap();
        let ids = ["run-0".to_owned(), "run-2".to_owned(), "missing".to_owned()]
            .into_iter()
            .collect();
        let mut calls = Vec::new();
        let found = try_get_many(&path, &ids, |record| {
            calls.push(record.decision.run_id);
            record.decision.created_at_ms
        })
        .unwrap();
        assert_eq!(calls, ["run-0", "run-2"]);
        assert_eq!(
            found,
            [("run-0".into(), 0), ("run-2".into(), 2)]
                .into_iter()
                .collect()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
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

    #[test]
    fn expired_lookup_does_not_create_sidecars_or_hide_corrupt_tails() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        assert!(try_get_until(&path, "run-0", Some(Instant::now())).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        let raw = format!("{}\n{{PRIVATE", serde_json::to_string(&record(0)).unwrap());
        fs::write(&path, &raw).unwrap();
        assert!(
            try_get_until(
                &path,
                "run-0",
                Some(Instant::now() + Duration::from_secs(2))
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        let lock = private_lock_options()
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().expect("failed lookup released its lock");
    }
}
