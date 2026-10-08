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
    for index in 0..5000 {
        fs::write(paths.directory.join(format!("unrelated-{index}")), "").unwrap();
    }
    // Test candidate rotation independently of host scheduling/IO speed. The
    // production two-second deadline has separate expiry/retention contracts.
    replay_with_budget(&paths, &storage, None, Duration::from_secs(30)).await;
    assert!(target.exists());
    // Cursor survives a new invocation; no corrupt file has to be deleted first.
    replay_with_budget(&paths, &storage, None, Duration::from_secs(30)).await;
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
    replay_with_budget(&paths, &storage, None, Duration::from_secs(30)).await;
    let after = fs::read_to_string(paths.directory.join("replay-state/cursor")).unwrap();
    assert!(after.starts_with("00000000-0000-0000-0000-"));
    save_journal(&target, observations).unwrap();
    fs::write(paths.directory.join("replay-state/cursor"), "invalid").unwrap();
    replay_with_budget(&paths, &storage, None, Duration::from_secs(30)).await;
    assert!(target.exists());
    replay_with_budget(&paths, &storage, Some(id), Duration::from_secs(30)).await;
    assert!(
        !target.exists(),
        "targeted recovery must not depend on the global cursor"
    );
}

#[test]
fn large_directory_scans_keep_bounded_windows_and_find_late_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..5000 {
        fs::write(dir.path().join(format!("unrelated-{index}")), "").unwrap();
    }
    for index in (0..300).rev() {
        fs::write(
            dir.path().join(format!(
                "jevia-events-00000000-0000-0000-0000-{index:012x}-a.jsonl"
            )),
            "",
        )
        .unwrap();
    }
    let id = "00000000-0000-0000-0000-000000000001";
    for suffix in ["b", "c"] {
        fs::write(
            dir.path().join(format!("jevia-events-{id}-{suffix}.jsonl")),
            "",
        )
        .unwrap();
    }
    let scan = |after| {
        replay_candidates(
            dir.path(),
            None,
            after,
            std::time::Instant::now() + Duration::from_secs(30),
        )
        .unwrap()
    };
    let first = scan(None);
    assert_eq!(first.len(), 128);
    assert_eq!(first[1].1.len(), 2);
    let next = scan(Some(&first.last().unwrap().0));
    assert_eq!(next[0].0, "00000000-0000-0000-0000-000000000080");
    let wrapped = scan(Some("ffffffff-ffff-ffff-ffff-ffffffffffff"));
    assert_eq!(wrapped, first);
    let targeted = replay_candidates(
        dir.path(),
        Some(id),
        None,
        std::time::Instant::now() + Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(targeted.len(), 1);
    assert_eq!(targeted[0].1.len(), 2);
    assert!(replay_candidates(dir.path(), None, None, std::time::Instant::now()).is_err());
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
