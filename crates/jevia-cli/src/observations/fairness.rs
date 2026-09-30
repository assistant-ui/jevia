use super::*;
use jevia_core::{Config, ExecutionEvidence, Outcome, RunLifecycle, RunState};

#[tokio::test]
async fn corrupt_prefix_cannot_starve_a_later_valid_journal() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(dir.path().into());
    let storage = Storage::open(&Config::default(), &paths, true)
        .await
        .unwrap();
    let mut record = crate::tests::sample_record();
    let id = "ffffffff-ffff-ffff-ffff-ffffffffffff";
    record.decision.run_id = id.into();
    record.lifecycle = Some(RunLifecycle {
        state: RunState::Completed,
        started_at_ms: Some(1),
        finished_at_ms: Some(2),
    });
    record.execution = Some(ExecutionEvidence {
        harness: "test".into(),
        model: "test".into(),
        duration_ms: 1,
        exit_code: Some(0),
        verification: None,
        observations: None,
    });
    storage.append(&record).await.unwrap();
    for index in 0..128 {
        fs::write(
            paths.directory.join(format!(
                "jevia-events-00000000-0000-0000-0000-{index:012x}-test.jsonl"
            )),
            "invalid",
        )
        .unwrap();
    }
    let target = paths
        .directory
        .join(format!("jevia-events-{id}-test.jsonl"));
    let observations = HarnessObservations {
        source: Some(ObservationSource::ClaudeHooks),
        status: Status::NoEvents,
        events: vec![],
        totals: None,
    };
    save_journal(&target, observations.clone()).unwrap();
    replay(&paths, &storage, None).await;
    assert!(target.exists());
    // Cursor survives a new invocation; no corrupt file has to be deleted first.
    replay(&paths, &storage, None).await;
    assert!(!target.exists());
    let saved = storage.get(id).await.unwrap();
    assert_eq!(
        saved.execution.unwrap().observations,
        Some(observations.clone())
    );
    assert_eq!(saved.outcome, Outcome::Unknown);
    assert!(
        paths
            .directory
            .join("jevia-events-00000000-0000-0000-0000-000000000000-test.jsonl")
            .exists()
    );
    // Completing a cycle still retries retained earlier candidates.
    let cursor = cursor::Cursor::open(&paths.directory).unwrap();
    cursor.advance(id).unwrap();
    drop(cursor);
    replay(&paths, &storage, None).await;
    let after = fs::read_to_string(paths.directory.join("replay-state/cursor")).unwrap();
    assert!(after.starts_with("00000000-0000-0000-0000-"));
    save_journal(&target, observations).unwrap();
    fs::write(paths.directory.join("replay-state/cursor"), "invalid").unwrap();
    replay(&paths, &storage, None).await;
    assert!(target.exists());
    replay(&paths, &storage, Some(id)).await;
    assert!(
        !target.exists(),
        "targeted recovery must not depend on the global cursor"
    );
}

#[test]
fn incomplete_directory_scans_never_treat_a_partial_set_as_unambiguous() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..4097 {
        fs::write(dir.path().join(format!("unrelated-{index}")), "").unwrap();
    }
    assert!(
        replay_candidates(
            dir.path(),
            None,
            std::time::Instant::now() + Duration::from_secs(10)
        )
        .is_err()
    );
}

#[test]
fn cursor_is_exclusive_private_and_rejects_malformed_state() {
    let dir = tempfile::tempdir().unwrap();
    let cursor = cursor::Cursor::open(dir.path()).unwrap();
    assert!(cursor::Cursor::open(dir.path()).is_err());
    let id = uuid::Uuid::new_v4().to_string();
    cursor.advance(&id).unwrap();
    assert_eq!(cursor.read().unwrap(), Some(id.clone()));
    drop(cursor);
    let cursor = cursor::Cursor::open(dir.path()).unwrap();
    assert_eq!(cursor.read().unwrap(), Some(id));
    fs::write(
        dir.path().join("replay-state/cursor"),
        "PRIVATE invalid content",
    )
    .unwrap();
    assert_eq!(
        cursor.read().unwrap_err().to_string(),
        "invalid replay cursor"
    );
    assert!(
        fs::read_to_string(dir.path().join(".gitignore"))
            .unwrap()
            .lines()
            .any(|line| line == "replay-state/")
    );
}
