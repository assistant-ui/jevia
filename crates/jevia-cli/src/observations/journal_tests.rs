use super::*;
use jevia_core::{Config, ExecutionEvidence, Outcome, RunLifecycle, RunState, StorageConfig};

fn snapshot() -> HarnessObservations {
    serde_json::from_value(json!({
        "source": "claude_hooks", "status": "recorded",
        "events": [{"kind": "turn_completed", "recorded_at_ms": 1}]
    }))
    .unwrap()
}

#[tokio::test]
async fn newer_journals_survive_capture_replay_and_finalization_unchanged() {
    for config in [
        Config::default(),
        Config {
            storage: StorageConfig::Sqlite {
                url: "sqlite://.jevia/test.db".into(),
            },
            ..Config::default()
        },
    ] {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().into());
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        for field in [
            "envelope",
            "snapshot",
            "event",
            "totals",
            "legacy_event",
            "discarded",
        ] {
            let mut record = crate::tests::sample_record();
            record.decision.run_id = uuid::Uuid::new_v4().to_string();
            record.outcome = Outcome::Unknown;
            record.lifecycle = Some(RunLifecycle {
                state: RunState::Completed,
                started_at_ms: Some(1),
                finished_at_ms: Some(2),
            });
            record.execution = Some(ExecutionEvidence {
                harness: "claude".into(),
                model: "requested".into(),
                duration_ms: 1,
                exit_code: Some(0),
                verification: None,
                observations: Some(HarnessObservations {
                    source: Some(ObservationSource::ClaudeHooks),
                    status: Status::NoEvents,
                    events: vec![],
                    totals: None,
                }),
            });
            storage.append(&record).await.unwrap();
            let journal = paths.directory.join(format!(
                "jevia-events-{}-test.jsonl",
                record.decision.run_id
            ));
            let known = snapshot();
            let mut wire = serde_json::to_value(Entry::Snapshot(known.clone())).unwrap();
            match field {
                "envelope" => wire["PRIVATE_FUTURE_FIELD"] = json!(42),
                "snapshot" => wire["event"]["PRIVATE_FUTURE_FIELD"] = json!({"cost": 42}),
                "event" => wire["event"]["events"][0]["PRIVATE_FUTURE_FIELD"] = json!(null),
                "totals" => {
                    wire["event"]["totals"] = serde_json::to_value(known.counts()).unwrap();
                    wire["event"]["totals"]["PRIVATE_FUTURE_FIELD"] = json!(42);
                }
                "legacy_event" => {
                    wire = json!({"type": "event", "event": wire["event"]["events"][0], "PRIVATE_FUTURE_FIELD": 42});
                }
                "discarded" => wire = json!({"type": "discarded", "PRIVATE_FUTURE_FIELD": 42}),
                _ => unreachable!(),
            }
            let raw = format!("{}\n", serde_json::to_string(&wire).unwrap());
            fs::write(&journal, &raw).unwrap();
            let error = read_snapshot_file(&journal).unwrap_err();
            assert!(!format!("{error:#}").contains("PRIVATE_FUTURE_FIELD"));
            receive_from(
                &journal,
                "claude_hooks",
                br#"{"hook_event_name":"Stop"}"#.as_slice(),
            );
            assert_eq!(fs::read_to_string(&journal).unwrap(), raw);
            assert!(!journal.with_extension("loss").exists());
            // A pre-existing loss marker must also remain recoverable.
            fs::write(journal.with_extension("loss"), []).unwrap();
            assert!(remove_saved_journal(&journal, &known).is_err());
            replay_with_budget(&paths, &storage, None, Duration::from_secs(30)).await;
            assert_eq!(storage.get(&record.decision.run_id).await.unwrap(), record);
            assert_eq!(fs::read_to_string(&journal).unwrap(), raw);
            assert!(journal.with_extension("loss").exists());
        }
    }
}

#[test]
fn compatible_legacy_journals_still_decode_and_finalize() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("jevia-events-test.jsonl");
    for raw in [
        "",
        "{\"type\":\"event\",\"event\":{\"kind\":\"turn_completed\",\"recorded_at_ms\":1}}\n",
        "{\"type\":\"discarded\"}\n{\"type\":\"truncated\"}\n",
        "{\"type\":\"snapshot\",\"event\":{\"source\":\"claude_hooks\",\"status\":\"no_events\",\"events\":[]}}\n",
    ] {
        fs::write(&journal, raw).unwrap();
        let decoded = read_snapshot_file(&journal).unwrap();
        remove_saved_journal(&journal, &decoded).unwrap();
        assert!(!journal.exists());
    }
    save_journal(&journal, snapshot()).unwrap();
    receive_inner(&journal, br#"{"hook_event_name":"Stop"}"#.as_slice()).unwrap();
    assert_eq!(read_snapshot_file(&journal).unwrap().event_count(), 2);
}

#[test]
fn duplicate_fields_are_rejected_without_replacing_the_journal() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("jevia-events-test.jsonl");
    let mut observations = snapshot();
    observations.totals = Some(observations.counts());
    let original = serde_json::to_string(&Entry::Snapshot(observations)).unwrap();
    for (field, replacement) in [
        (
            "\"type\":\"snapshot\"",
            "\"type\":\"discarded\",\"type\":\"snapshot\"",
        ),
        (
            "\"status\":\"recorded\"",
            "\"status\":\"no_events\",\"status\":\"recorded\"",
        ),
        (
            "\"recorded_at_ms\":1",
            "\"recorded_at_ms\":0,\"recorded_at_ms\":1",
        ),
        (
            "\"discarded_inputs\":0",
            "\"discarded_inputs\":0,\"discarded_inputs\":0",
        ),
    ] {
        assert!(original.contains(field));
        let raw = original.replacen(field, replacement, 1);
        fs::write(&journal, &raw).unwrap();
        assert!(read_snapshot_file(&journal).is_err());
        assert!(receive_inner(&journal, br#"{"hook_event_name":"Stop"}"#.as_slice()).is_err());
        assert_eq!(fs::read_to_string(&journal).unwrap(), raw);
    }
}
