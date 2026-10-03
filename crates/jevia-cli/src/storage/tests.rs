use super::*;
use jevia_core::{DecisionSource, RouteDecision};
use std::collections::BTreeMap;

fn sample(id: &str) -> RouteRecord {
    RouteRecord::new(
        RouteDecision {
            run_id: id.to_owned(),
            tier: "fast".into(),
            suggested_tier: "fast".into(),
            confidence: 0.9,
            probabilities: BTreeMap::new(),
            fallback_applied: false,
            jev_model: "test".into(),
            created_at_ms: 1,
            source: DecisionSource::Live,
        },
        Some("sensitive task".into()),
    )
}

fn sqlite_config() -> Config {
    Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/history with spaces.db".into(),
        },
        ..Config::default()
    }
}

#[test]
fn dropped_live_checkpoint_cannot_write_after_its_worker_deadline() {
    use jevia_core::{HarnessEvent, HarnessEventKind, ObservationSource, ObservationStatus};
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let original = runtime.block_on(async {
        let storage = Storage::open(&Config::default(), &paths, true)
            .await
            .unwrap();
        storage.append(&sample("checkpoint")).await.unwrap();
        let mut observations = HarnessObservations {
            source: Some(ObservationSource::ClaudeHooks),
            status: ObservationStatus::NoEvents,
            events: vec![],
            totals: None,
        };
        storage
            .state(
                "checkpoint",
                RunState::Running,
                Outcome::Unknown,
                Some(ExecutionEvidence {
                    observations: Some(observations.clone()),
                    harness: "test".into(),
                    model: "test".into(),
                    duration_ms: 0,
                    exit_code: None,
                    verification: None,
                }),
            )
            .await
            .unwrap();
        let original = std::fs::read(&paths.runs).unwrap();
        observations.observe(HarnessEvent {
            kind: HarnessEventKind::TurnCompleted,
            recorded_at_ms: 1,
            session_id: None,
            agent_id: None,
            model: None,
            previous_model: None,
            tool_name: None,
        });

        // Occupy the only worker: cancellation must also stop work that has not
        // started yet, not just stop waiting for its JoinHandle.
        let (started, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let holder = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_millis(20);
        let result = tokio::time::timeout_at(
            deadline.into(),
            storage.checkpoint_observations("checkpoint", observations, true, Some(deadline)),
        )
        .await;
        release.send(()).unwrap();
        holder.await.unwrap();
        assert!(
            result.is_err(),
            "the queued worker outlives its async waiter"
        );
        original
    });
    // Runtime shutdown joins even detached blocking work before inspecting state.
    drop(runtime);
    assert_eq!(std::fs::read(&paths.runs).unwrap(), original);
    let saved = store::record_state(
        &paths.runs,
        "checkpoint",
        RunState::Completed,
        Outcome::Unknown,
        None,
    )
    .unwrap();
    assert_eq!(saved.lifecycle.unwrap().state, RunState::Completed);
}

#[tokio::test]
async fn jsonl_checkpoints_skip_busy_history_and_reject_late_supervisor_writes() {
    use jevia_core::{ObservationSource, ObservationStatus};
    use std::time::{Duration, Instant};
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&Config::default(), &paths, true)
        .await
        .unwrap();
    storage.append(&sample("checkpoint")).await.unwrap();
    let observations = HarnessObservations {
        source: Some(ObservationSource::ClaudeHooks),
        status: ObservationStatus::NoEvents,
        events: vec![],
        totals: None,
    };
    storage
        .state(
            "checkpoint",
            RunState::Running,
            Outcome::Unknown,
            Some(ExecutionEvidence {
                observations: Some(observations.clone()),
                harness: "test".into(),
                model: "test".into(),
                duration_ms: 0,
                exit_code: None,
                verification: None,
            }),
        )
        .await
        .unwrap();
    let file = File::options()
        .read(true)
        .write(true)
        .open(paths.runs.with_extension("lock"))
        .unwrap();
    file.lock().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = lease::FileLock::new(file);
        // Safety bound lets the pre-fix regression fail rather than deadlock.
        let _ = wait.recv_timeout(Duration::from_secs(3));
    });
    let started = Instant::now();
    let result = storage
        .checkpoint_observations("checkpoint", observations.clone(), true, None)
        .await;
    let elapsed = started.elapsed();
    let _ = release.send(());
    holder.join().unwrap();
    assert!(
        result.is_err(),
        "busy history must leave the journal for retry"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "checkpoint waited on the lock"
    );
    storage
        .checkpoint_observations("checkpoint", observations.clone(), true, None)
        .await
        .unwrap();
    storage
        .state("checkpoint", RunState::TimedOut, Outcome::Unknown, None)
        .await
        .unwrap();
    assert!(
        storage
            .checkpoint_observations("checkpoint", observations.clone(), true, None)
            .await
            .is_err()
    );
    // Explicit recovery/replay can still restore observations on terminal runs.
    storage
        .checkpoint_observations("checkpoint", observations, false, None)
        .await
        .unwrap();
    assert_eq!(
        storage
            .get("checkpoint")
            .await
            .unwrap()
            .lifecycle
            .unwrap()
            .state,
        RunState::TimedOut
    );
}

#[tokio::test]
async fn invalid_decisions_are_rejected_before_jsonl_or_sqlite_writes() {
    for config in [Config::default(), sqlite_config()] {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().into());
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        let mut bad = sample("private-run");
        bad.decision
            .probabilities
            .insert("private-tier".into(), 2.0);
        let error = storage.append(&bad).await.unwrap_err();
        assert!(!format!("{error:#}").contains("private-"));
        assert!(storage.recent(10, false).await.unwrap().is_empty());
    }
}

fn postgres_config() -> Config {
    Config {
        storage: StorageConfig::Postgres {
            url_env: "JEVIA_TEST_POSTGRES_URL".into(),
            project: format!("test-{}", uuid::Uuid::new_v4()),
            allow_insecure_localhost: true,
        },
        ..Config::default()
    }
}

async fn application_recording_contract(config: Config) {
    use jevia_core::{ExecutionRecording, HarnessEvent, HarnessEventKind, ObservationSource};
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&config, &paths, true).await.unwrap();
    let input = ExecutionRecording {
        harness: "my-app".into(),
        model: "requested".into(),
        duration_ms: 12,
        exit_code: Some(1),
        events: vec![HarnessEvent {
            kind: HarnessEventKind::ToolFailed,
            recorded_at_ms: 7,
            session_id: None,
            agent_id: None,
            model: Some("observed".into()),
            previous_model: None,
            tool_name: Some("test".into()),
        }],
    };
    storage.append(&sample("sdk")).await.unwrap();
    let saved = storage
        .record_application("sdk", input.clone())
        .await
        .unwrap();
    assert_eq!(saved.outcome, Outcome::Unknown);
    assert!(saved.outcome_evidence.is_none());
    assert_eq!(saved.lifecycle.as_ref().unwrap().state, RunState::Completed);
    assert_eq!(
        saved
            .execution
            .as_ref()
            .unwrap()
            .observations
            .as_ref()
            .unwrap()
            .source,
        Some(ObservationSource::Application)
    );
    assert_eq!(
        storage
            .record_application("sdk", input.clone())
            .await
            .unwrap(),
        saved
    );
    let mut conflicting = input.clone();
    conflicting.duration_ms += 1;
    assert!(
        storage
            .record_application("sdk", conflicting)
            .await
            .is_err()
    );
    assert_eq!(storage.get("sdk").await.unwrap(), saved);
    assert_eq!(storage.routing_history(10).await.unwrap(), vec![saved]);
    let feedback = storage
        .outcome("sdk", Outcome::Success, None)
        .await
        .unwrap();
    assert_eq!(
        storage
            .record_application("sdk", input.clone())
            .await
            .unwrap(),
        feedback
    );
    // Feedback before recording is equally optional and never erased by the snapshot.
    storage.append(&sample("feedback-first")).await.unwrap();
    storage
        .outcome("feedback-first", Outcome::Failure, None)
        .await
        .unwrap();
    assert_eq!(
        storage
            .record_application("feedback-first", input.clone())
            .await
            .unwrap()
            .outcome,
        Outcome::Failure
    );
    storage.append(&sample("owned")).await.unwrap();
    let guard = storage.execution_guard("owned").await.unwrap();
    assert!(
        storage
            .record_application("owned", input.clone())
            .await
            .is_err()
    );
    storage
        .state("owned", RunState::Running, Outcome::Unknown, None)
        .await
        .unwrap();
    drop(guard);
    assert!(
        storage
            .record_application("owned", input.clone())
            .await
            .is_err()
    );
    storage.append(&sample("invalid")).await.unwrap();
    let mut invalid = input.clone();
    invalid.events[0].model = Some("PRIVATE prompt with spaces".into());
    assert!(
        storage
            .record_application("invalid", invalid)
            .await
            .is_err()
    );
    assert!(storage.get("invalid").await.unwrap().execution.is_none());
    storage.append(&sample("empty")).await.unwrap();
    let mut empty = input;
    empty.events.clear();
    empty.exit_code = None;
    let saved = storage.record_application("empty", empty).await.unwrap();
    assert_eq!(
        saved.execution.unwrap().observations.unwrap().event_count(),
        0
    );
}

#[tokio::test]
async fn jsonl_and_sqlite_application_recording() {
    application_recording_contract(Config::default()).await;
    application_recording_contract(sqlite_config()).await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_application_recording() {
    application_recording_contract(postgres_config()).await;
}

async fn terminal_snapshot_contract(config: Config) {
    use jevia_core::{HarnessEvent, HarnessEventKind, ObservationSource, ObservationStatus};
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&config, &paths, true).await.unwrap();
    for terminal in [
        RunState::Completed,
        RunState::Cancelled,
        RunState::TimedOut,
        RunState::LaunchFailed,
    ] {
        let id = format!("{terminal:?}");
        storage.append(&sample(&id)).await.unwrap();
        let mut observed = HarnessObservations {
            source: Some(ObservationSource::ClaudeHooks),
            status: ObservationStatus::NoEvents,
            events: vec![],
            totals: None,
        };
        let mut execution = ExecutionEvidence {
            harness: "agent".into(),
            model: "requested".into(),
            duration_ms: 0,
            exit_code: None,
            verification: None,
            observations: Some(observed.clone()),
        };
        let _guard = storage.execution_guard(&id).await.unwrap();
        storage
            .state(
                &id,
                RunState::Running,
                Outcome::Unknown,
                Some(execution.clone()),
            )
            .await
            .unwrap();
        observed.observe(HarnessEvent {
            kind: HarnessEventKind::TurnCompleted,
            recorded_at_ms: 1,
            session_id: None,
            agent_id: None,
            model: Some("observed".into()),
            previous_model: None,
            tool_name: None,
        });
        storage
            .checkpoint_observations(&id, observed, true, None)
            .await
            .unwrap();
        execution.observations.as_mut().unwrap().status = ObservationStatus::Partial;
        let saved = storage
            .state(&id, terminal, Outcome::Unknown, Some(execution))
            .await
            .unwrap();
        assert_eq!(saved.lifecycle.unwrap().state, terminal);
        assert_eq!(saved.outcome, Outcome::Unknown);
        let observations = saved.execution.unwrap().observations.unwrap();
        assert_eq!(observations.event_count(), 1);
        assert_eq!(observations.status, ObservationStatus::Partial);
        assert_eq!(observations.events[0].model.as_deref(), Some("observed"));
    }
}

#[tokio::test]
async fn jsonl_and_sqlite_terminal_writes_preserve_checkpoints() {
    terminal_snapshot_contract(Config::default()).await;
    terminal_snapshot_contract(sqlite_config()).await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_terminal_writes_preserve_checkpoints() {
    terminal_snapshot_contract(postgres_config()).await;
}

async fn observation_history_contract(config: Config) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&config, &paths, true).await.unwrap();
    storage.append(&sample("known")).await.unwrap();
    storage
        .outcome("known", Outcome::Success, None)
        .await
        .unwrap();
    for id in ["observed-old", "observed-new"] {
        storage.append(&sample(id)).await.unwrap();
        storage
            .state(id, RunState::Running, Outcome::Unknown, None)
            .await
            .unwrap();
        storage
            .state(
                id,
                RunState::Completed,
                Outcome::Unknown,
                Some(ExecutionEvidence {
                    observations: None,
                    harness: "agent".into(),
                    model: "requested".into(),
                    duration_ms: 42,
                    exit_code: Some(0),
                    verification: None,
                }),
            )
            .await
            .unwrap();
    }
    // More than one database page of pending runs must not starve either window.
    for i in 0..130 {
        storage
            .append(&sample(&format!("pending-{i}")))
            .await
            .unwrap();
    }
    let history = storage.routing_history(1).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|r| r.decision.run_id.as_str())
            .collect::<Vec<_>>(),
        ["known", "observed-new"]
    );
    assert_eq!(history[1].outcome, Outcome::Unknown);
    assert!(storage.routing_history(0).await.unwrap().is_empty());
    storage
        .outcome("observed-new", Outcome::Failure, None)
        .await
        .unwrap();
    let history = storage.routing_history(1).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|r| r.decision.run_id.as_str())
            .collect::<Vec<_>>(),
        ["observed-new", "observed-old"]
    );
}

#[tokio::test]
async fn jsonl_and_sqlite_observation_history() {
    observation_history_contract(Config::default()).await;
    observation_history_contract(sqlite_config()).await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_observation_history() {
    observation_history_contract(postgres_config()).await;
}

async fn external_completion_contract(config: Config) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&config, &paths, true).await.unwrap();
    for id in ["a", "b", "pending", "guarded", "active"] {
        storage.append(&sample(id)).await.unwrap();
    }
    let original = storage.get("a").await.unwrap();
    assert!(
        storage
            .complete_external("a", Outcome::Success, None, false)
            .await
            .is_err()
    );
    assert_eq!(storage.get("a").await.unwrap(), original);
    let guard = storage.execution_guard("guarded").await.unwrap();
    assert!(
        storage
            .complete_external("guarded", Outcome::Success, None, true)
            .await
            .is_err()
    );
    drop(guard);
    storage
        .state("active", RunState::Running, Outcome::Unknown, None)
        .await
        .unwrap();
    assert!(
        storage
            .complete_external("active", Outcome::Success, None, true)
            .await
            .is_err()
    );
    storage.outcome("a", Outcome::Failure, None).await.unwrap();
    let before = storage.get("a").await.unwrap();
    assert!(
        storage
            .complete_external("a", Outcome::Success, None, true)
            .await
            .is_err()
    );
    assert_eq!(storage.get("a").await.unwrap(), before);
    let done = storage
        .complete_external("a", Outcome::Success, Some("external tests passed"), true)
        .await
        .unwrap();
    assert_eq!(done.lifecycle.as_ref().unwrap().state, RunState::Completed);
    assert!(done.lifecycle.as_ref().unwrap().started_at_ms.is_none());
    assert!(done.lifecycle.as_ref().unwrap().finished_at_ms.is_some());
    assert!(done.execution.is_none());
    assert_eq!(
        done.outcome_evidence.as_ref().unwrap().source,
        jevia_core::OutcomeSource::Manual
    );
    assert!(done.is_learning_evidence());
    assert_eq!(done.feedback.len(), 2);
    assert!(
        storage
            .complete_external("a", Outcome::Success, None, true)
            .await
            .is_err()
    );
    assert_eq!(storage.get("a").await.unwrap(), done);
    storage
        .complete_external("b", Outcome::Unknown, None, true)
        .await
        .unwrap();
    assert!(!storage.get("b").await.unwrap().is_learning_evidence());
    let preview = storage.archive(&paths, 1, false).await.unwrap();
    assert_eq!(preview.archived_records, 1);
    assert_eq!(storage.recent(10, false).await.unwrap().len(), 5);
    assert_eq!(
        storage
            .archive(&paths, 1, true)
            .await
            .unwrap()
            .archived_records,
        1
    );
    assert!(storage.get("a").await.is_err());
    assert_eq!(
        storage
            .get("pending")
            .await
            .unwrap()
            .lifecycle
            .unwrap()
            .state,
        RunState::Routed
    );
    assert_eq!(
        storage
            .get("active")
            .await
            .unwrap()
            .lifecycle
            .unwrap()
            .state,
        RunState::Running
    );
    storage.append(&sample("racing")).await.unwrap();
    let (one, two) = tokio::join!(
        storage.complete_external("racing", Outcome::Success, None, true),
        storage.complete_external("racing", Outcome::Success, None, true),
    );
    assert_ne!(one.is_ok(), two.is_ok(), "exactly one completion wins");
    assert_eq!(storage.get("racing").await.unwrap().feedback.len(), 1);
}

#[tokio::test]
async fn jsonl_external_completion_contract() {
    external_completion_contract(Config::default()).await;
}

#[tokio::test]
async fn sqlite_external_completion_contract() {
    external_completion_contract(sqlite_config()).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_external_completion_contract() {
    external_completion_contract(postgres_config()).await;
}

#[tokio::test]
#[cfg(unix)]
async fn export_refuses_symlink_and_hardlink_destinations_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    let storage = Storage::open(&Config::default(), &paths, true)
        .await
        .unwrap();
    storage.append(&sample("one")).await.unwrap();
    let original = std::fs::read(&paths.runs).unwrap();
    let linked = directory.path().join("linked.jsonl");
    let hardlinked = directory.path().join("hardlinked.jsonl");
    let dangling = directory.path().join("dangling.jsonl");
    let absent = directory.path().join("absent.jsonl");
    symlink(&paths.runs, &linked).unwrap();
    std::fs::hard_link(&paths.runs, &hardlinked).unwrap();
    symlink(&absent, &dangling).unwrap();
    for path in [&linked, &hardlinked, &dangling, &paths.runs] {
        assert!(storage.export(path).await.is_err());
    }
    assert_eq!(std::fs::read(paths.runs).unwrap(), original);
    assert!(!absent.exists());
}

async fn contract(config: Config) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().to_owned());
    let first = Storage::open(&config, &paths, true).await.unwrap();
    let second = Storage::open(&config, &paths, false).await.unwrap();
    assert_eq!(first.check().await.unwrap(), 0);
    let a = sample("a");
    let mut b = sample("b");
    b.decision.created_at_ms = 0; // Append order, not a remote machine's clock.
    first.append(&a).await.unwrap();
    second.append(&b).await.unwrap();
    assert!(second.append(&a).await.is_err());
    assert_eq!(first.recent(1, false).await.unwrap(), vec![b]);
    assert!(first.recent(0, false).await.unwrap().is_empty());
    assert!(first.recent(20, true).await.unwrap().is_empty());
    let (one, two) = tokio::join!(
        first.outcome("a", Outcome::Success, Some("first")),
        second.outcome("a", Outcome::Success, Some("second")),
    );
    one.unwrap();
    two.unwrap();
    let a = first.get("a").await.unwrap();
    assert_eq!(a.feedback.len(), 2); // No lost feedback under concurrent writers.
    assert!(a.is_learning_evidence());
    assert_eq!(second.recent(1, true).await.unwrap(), vec![a]);
    assert!(second.outcome("a", Outcome::Failure, None).await.is_err());
    second
        .outcome("a", Outcome::Failure, Some("verified regression"))
        .await
        .unwrap();
    assert_eq!(first.get("a").await.unwrap().feedback.len(), 3);

    let guard = first.execution_guard("b").await.unwrap();
    first
        .state("b", RunState::Running, Outcome::Unknown, None)
        .await
        .unwrap();
    assert!(second.recover("b", true).await.is_err());
    assert!(second.outcome("b", Outcome::Success, None).await.is_err());
    assert!(
        second
            .state("b", RunState::Completed, Outcome::Success, None)
            .await
            .is_err()
    );
    drop(guard);
    // A dropped PostgreSQL connection is released asynchronously at the server.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if second.recover("b", true).await.is_ok() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "execution lock was not released"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // Recovery is terminal and fences even the former owner's late write.
    assert!(
        first
            .state("b", RunState::Completed, Outcome::Success, None)
            .await
            .is_err()
    );
    assert_eq!(second.get("b").await.unwrap().outcome, Outcome::Unknown);
    assert_eq!(first.check().await.unwrap(), 2);
    assert_eq!(first.recent(usize::MAX, false).await.unwrap().len(), 2);

    let export = directory.path().join("snapshot.jsonl");
    assert_eq!(first.export(&export).await.unwrap(), 2);
    let original = std::fs::read(&export).unwrap();
    assert!(first.export(&export).await.is_err());
    assert_eq!(std::fs::read(&export).unwrap(), original);
    assert_eq!(
        first.import_jsonl(export.clone(), false).await.unwrap(),
        (0, 2)
    );
    assert_eq!(first.import_jsonl(export, true).await.unwrap(), (0, 2));
}

#[tokio::test]
async fn sqlite_contract() {
    contract(sqlite_config()).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_contract() {
    contract(postgres_config()).await;
}

#[tokio::test]
async fn sqlite_import_is_atomic_previewed_and_preserves_original() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(dir.path().into());
    let storage = Storage::open(&sqlite_config(), &paths, true).await.unwrap();
    let source = dir.path().join("import.jsonl");
    store::append(&source, &sample("a")).unwrap();
    store::append(&source, &sample("b")).unwrap();
    let original = std::fs::read(&source).unwrap();
    assert_eq!(
        storage.import_jsonl(source.clone(), false).await.unwrap(),
        (2, 0)
    );
    assert_eq!(storage.check().await.unwrap(), 0);
    assert_eq!(
        storage.import_jsonl(source.clone(), true).await.unwrap(),
        (2, 0)
    );
    assert_eq!(
        storage.import_jsonl(source.clone(), true).await.unwrap(),
        (0, 2)
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);

    let conflict = dir.path().join("conflict.jsonl");
    store::append(&conflict, &sample("new-before-conflict")).unwrap();
    let mut changed = sample("a");
    changed.task = Some("different content".into());
    store::append(&conflict, &changed).unwrap();
    assert!(storage.import_jsonl(conflict, true).await.is_err());
    assert!(storage.get("new-before-conflict").await.is_err());
    assert_eq!(storage.check().await.unwrap(), 2);

    store::append(&source, &sample("a")).unwrap();
    assert!(storage.import_jsonl(source, true).await.is_err());
    let active = dir.path().join("active.jsonl");
    let mut record = sample("active");
    record.lifecycle.as_mut().unwrap().state = RunState::Running;
    store::append(&active, &record).unwrap();
    assert!(storage.import_jsonl(active, true).await.is_err());
}

#[tokio::test]
async fn sqlite_is_private_and_schema_checks_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(dir.path().into());
    let config = sqlite_config();
    assert!(Storage::open(&config, &paths, false).await.is_err());
    let first = Storage::open(&config, &paths, true).await.unwrap();
    first.append(&sample("one")).await.unwrap();
    let reopened = Storage::open(&config, &paths, true).await.unwrap();
    assert_eq!(reopened.check().await.unwrap(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(paths.directory.join("history with spaces.db"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let Storage::Database(database) = first else {
        unreachable!()
    };
    database.set_schema_version_for_test(999).await;
    assert!(Storage::open(&config, &paths, false).await.is_err());
    assert!(Storage::open(&config, &paths, true).await.is_err());
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_projects_are_isolated_and_recovery_requires_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(dir.path().into());
    let a = Storage::open(&postgres_config(), &paths, true)
        .await
        .unwrap();
    let b = Storage::open(&postgres_config(), &paths, true)
        .await
        .unwrap();
    a.append(&sample("same-id")).await.unwrap();
    assert!(b.get("same-id").await.is_err());
    b.append(&sample("same-id")).await.unwrap();
    a.outcome("same-id", Outcome::Success, None).await.unwrap();
    assert_eq!(b.get("same-id").await.unwrap().outcome, Outcome::Unknown);
    b.state("same-id", RunState::Running, Outcome::Unknown, None)
        .await
        .unwrap();
    assert!(b.recover("same-id", false).await.is_err());
    b.recover("same-id", true).await.unwrap();
    assert_eq!(a.get("same-id").await.unwrap().outcome, Outcome::Success);
}
