use super::*;

/// Validate the full stream, keeping independent bounded windows under one shared lock.
/// A feedback writer cannot move a run between categories during this snapshot.
#[cfg(test)]
pub fn routing_history(path: &Path, limit: usize) -> Result<(Vec<RouteRecord>, Vec<RouteRecord>)> {
    routing_history_with(path, limit, Clone::clone)
}

pub fn routing_history_with<T>(
    path: &Path,
    limit: usize,
    project: impl Fn(&RouteRecord) -> T,
) -> Result<(Vec<T>, Vec<T>)> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let Some(file) = crate::regular_file::open_optional(path)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut reader = BufReader::new(file);
    let mut known = VecDeque::new();
    let mut observed = VecDeque::new();
    read_records_with_positions(&mut reader, path, None, |record, position| {
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
                window.push_back(Candidate::Deferred(position));
            } else {
                // Small histories stay one-pass. Once a window fills, record
                // offsets instead of repeatedly projecting soon-evicted rows.
                window.push_back(Candidate::Ready(project(&record)));
            }
        }
        Ok(())
    })?;
    // Same file handle and lock: no writer can move rows between windows or
    // change offsets. Only selected deferred records are reread, not the file.
    Ok((
        finish_window(&mut reader, path, known, &project)?,
        finish_window(&mut reader, path, observed, &project)?,
    ))
}

enum Candidate<T> {
    Ready(T),
    Deferred(RecordPosition),
}

fn finish_window<T>(
    reader: &mut BufReader<File>,
    path: &Path,
    candidates: VecDeque<Candidate<T>>,
    project: &impl Fn(&RouteRecord) -> T,
) -> Result<Vec<T>> {
    let mut bytes = Vec::new();
    candidates
        .into_iter()
        .map(|candidate| match candidate {
            Candidate::Ready(value) => Ok(value),
            Candidate::Deferred(position) => {
                reader
                    .seek(SeekFrom::Start(position.offset))
                    .context("could not seek selected history record")?;
                if !read_line_until(reader, &mut bytes, None)? {
                    bail!("selected history record disappeared; no partial history returned");
                }
                let record = decode_record_line(&bytes, position.line, path)?
                    .context("selected history record is empty; no partial history returned")?;
                Ok(project(&record))
            }
        })
        .collect()
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

    #[test]
    fn projection_work_is_bounded_without_retaining_full_history_payloads() {
        use std::cell::Cell;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let mut records = Vec::new();
        let mut raw = "\n\u{2003}\n".to_owned();
        for id in 0..500 {
            let mut row = record(id % 10);
            row.decision.run_id = format!("run-{id}");
            row.task = Some(format!("Unicode 🦀 {id}: {}", "x".repeat(4096)));
            if id % 2 == 0 {
                row.outcome = Outcome::Success;
                row.outcome_evidence = Some(OutcomeEvidence {
                    source: OutcomeSource::Manual,
                    recorded_at_ms: 1,
                });
            }
            raw.push_str(&serde_json::to_string(&row).unwrap());
            if id != 499 {
                raw.push_str("\r\n \t\n");
            }
            records.push(row);
        }
        fs::write(&path, &raw).unwrap(); // A valid unterminated final record.
        for limit in [0, 1, 20, 300] {
            let calls = Cell::new(0);
            let (known, observed) = routing_history_with(&path, limit, |record| {
                calls.set(calls.get() + 1);
                (record.decision.run_id.clone(), record.task.clone())
            })
            .unwrap();
            assert!(
                calls.get() <= 4 * limit,
                "projected {} records for limit {limit}",
                calls.get()
            );
            if limit >= 250 {
                assert_eq!(
                    calls.get(),
                    records.len(),
                    "small histories need no second projection"
                );
            }
            for (actual, learning) in [(known, true), (observed, false)] {
                let eligible: Vec<_> = records
                    .iter()
                    .filter(|r| r.is_learning_evidence() == learning)
                    .collect();
                let expected: Vec<_> = eligible[eligible.len().saturating_sub(limit)..]
                    .iter()
                    .map(|r| (r.decision.run_id.clone(), r.task.clone()))
                    .collect();
                assert_eq!(actual, expected);
            }
        }
        assert_eq!(fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn deferred_projections_keep_the_snapshot_lock_and_release_it_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        for id in 0..6 {
            append(&path, &record(id)).unwrap();
        }
        routing_history_with(&path, 1, |_| {
            let writer = private_lock_options()
                .open(path.with_extension("lock"))
                .unwrap();
            assert!(matches!(
                writer.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
        })
        .unwrap();
        drop(acquire_lock(&path, LockMode::Exclusive).unwrap());
        // A corrupt tail must fail the entire selection, then release ownership.
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{original}{{PRIVATE invalid")).unwrap();
        assert!(routing_history_with(&path, 1, |_| ()).is_err());
        acquire_lock(&path, LockMode::Exclusive).unwrap();
    }
}
