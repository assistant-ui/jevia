use super::*;

/// Read/validate once, keeping independent bounded windows under one shared lock.
/// A feedback writer cannot move a run between categories during this snapshot.
pub fn routing_history(path: &Path, limit: usize) -> Result<(Vec<RouteRecord>, Vec<RouteRecord>)> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let mut known = VecDeque::new();
    let mut observed = VecDeque::new();
    read_records(path, |record| {
        let window = if record.is_learning_evidence() {
            &mut known
        } else if record.is_execution_observation() {
            &mut observed
        } else {
            return Ok(());
        };
        if limit != 0 {
            if window.len() == limit {
                window.pop_front();
            }
            window.push_back(record);
        }
        Ok(())
    })?;
    Ok((known.into(), observed.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: usize) -> RouteRecord {
        serde_json::from_value(json!({
            "schema_version": 6, "run_id": format!("run-{id}"), "tier": "fast", "suggested_tier": "fast",
            "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
            "created_at_ms": 10_usize.saturating_sub(id), "task": null, "outcome": "unknown",
            "lifecycle": {"state": "completed"},
            "execution": {"harness": "test", "model": "test", "duration_ms": 1, "exit_code": 0}
        })).unwrap()
    }

    #[test]
    fn independent_windows_keep_append_order_and_validate_the_whole_stream() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        assert_eq!(routing_history(&path, 1).unwrap(), (vec![], vec![]));
        for id in 0..6 {
            let mut row = record(id);
            if id.is_multiple_of(2) {
                row.outcome = Outcome::Success;
                row.outcome_evidence = Some(OutcomeEvidence {
                    source: OutcomeSource::Manual,
                    recorded_at_ms: 1,
                });
            }
            append(&path, &row).unwrap();
        }
        let (known, observed) = routing_history(&path, 2).unwrap();
        assert_eq!(
            known
                .iter()
                .map(|r| r.decision.run_id.as_str())
                .collect::<Vec<_>>(),
            ["run-2", "run-4"]
        );
        assert_eq!(observed, [record(3), record(5)]);
        assert_eq!(routing_history(&path, 0).unwrap(), (vec![], vec![]));
        // Bounds must not hide a corrupt prefix/suffix, even with history disabled.
        let valid = fs::read_to_string(&path).unwrap();
        for raw in [
            format!("{{private-invalid\n{valid}"),
            format!("{valid}{{private-invalid"),
        ] {
            fs::write(&path, raw).unwrap();
            for limit in [0, 1, 20] {
                let error = routing_history(&path, limit).unwrap_err();
                assert!(!format!("{error:#}").contains("private-invalid"));
            }
        }
    }
}
