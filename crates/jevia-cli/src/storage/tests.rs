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
