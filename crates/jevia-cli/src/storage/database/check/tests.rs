use super::*;
use crate::{paths::ProjectPaths, storage::Storage};
use jevia_core::{Config, RouteRecord, StorageConfig};
use serde_json::json;

const RECORDS: usize = PAGE_SIZE as usize * 2 + 5;

async fn unsafe_numbers(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../jevia-core/tests/fixtures/numeric-record.json"
    )))
    .unwrap();
    let valid: RouteRecord = serde_json::from_value(fixture.clone()).unwrap();
    f.storage.append(&valid).await.unwrap();
    for path in [
        "/created_at_ms",
        "/lifecycle/started_at_ms",
        "/lifecycle/finished_at_ms",
        "/outcome_evidence/recorded_at_ms",
        "/feedback/0/recorded_at_ms",
        "/execution/duration_ms",
        "/execution/verification/duration_ms",
        "/execution/observations/events/0/recorded_at_ms",
    ] {
        let mut invalid = fixture.clone();
        *invalid.pointer_mut(path).unwrap() = json!(jevia_core::MAX_SAFE_INTEGER + 2);
        let raw = invalid.to_string();
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1")
            .bind(&f.db().project)
            .bind(&raw)
            .execute(&f.db().pool)
            .await
            .unwrap();
        for result in [
            f.storage.check_deep().await,
            f.db().export(std::io::sink()).await,
        ] {
            let error = format!("{:#}", result.unwrap_err());
            assert!(!error.contains("PRIVATE"));
            assert!(!error.contains("9007199254740993"));
        }
        assert!(f.storage.get("numeric-boundary").await.is_err());
        assert!(f.storage.recent(10, false).await.is_err());
        let after: String = sqlx::query_scalar("SELECT record FROM jevia_runs WHERE project = $1")
            .bind(&f.db().project)
            .fetch_one(&f.db().pool)
            .await
            .unwrap();
        assert_eq!(
            after, raw,
            "invalid stored values must never be rounded or repaired"
        );
        assert_eq!(f.counter().await, 1);
    }
    let mut invalid = valid;
    invalid.decision.run_id = "another-run".into();
    invalid.decision.created_at_ms = jevia_core::MAX_SAFE_INTEGER + 1;
    assert!(f.storage.append(&invalid).await.is_err());
    assert_eq!(f.counter().await, 1);
}

#[tokio::test]
async fn sqlite_unsafe_history_numbers_are_rejected_without_repair() {
    unsafe_numbers(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_unsafe_history_numbers_are_rejected_without_repair() {
    unsafe_numbers(true).await;
}

struct Fixture {
    _dir: tempfile::TempDir,
    storage: Storage,
}

impl Fixture {
    async fn new(postgres: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(dir.path().into());
        let config = Config {
            storage: if postgres {
                StorageConfig::Postgres {
                    url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                    project: format!("deep-check-{}", uuid::Uuid::new_v4()),
                    allow_insecure_localhost: true,
                }
            } else {
                StorageConfig::Sqlite {
                    url: "sqlite://.jevia/check.db".into(),
                }
            },
            ..Config::default()
        };
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        Self { _dir: dir, storage }
    }

    fn db(&self) -> &Database {
        let Storage::Database(db) = &self.storage else {
            unreachable!()
        };
        db
    }

    async fn seed(&self) {
        let mut tx = self.db().write().await.unwrap();
        for index in 0..RECORDS {
            self.db().insert(&mut tx, &record(index)).await.unwrap();
        }
        tx.commit().await.unwrap();
    }

    async fn counter(&self) -> i64 {
        sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = $1")
            .bind(&self.db().project)
            .fetch_one(&self.db().pool)
            .await
            .unwrap()
    }
}

fn record(index: usize) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 1 + index % 3, "run_id": format!("run-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": RECORDS.saturating_sub(index), "task": "private-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": {"state": if index.is_multiple_of(5) { "running" } else { "completed" }, "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()
}

#[tokio::test]
async fn sqlite_external_completion_refuses_stale_supervisor_ownership() {
    let f = Fixture::new(false).await;
    let mut pending = record(0);
    pending.lifecycle = Some(Default::default());
    pending.execution = None;
    pending.outcome_evidence = None;
    f.storage.append(&pending).await.unwrap();
    sqlx::query("UPDATE jevia_runs SET owner = 'stale-owner' WHERE project = $1")
        .bind(&f.db().project)
        .execute(&f.db().pool)
        .await
        .unwrap();
    assert!(
        f.storage
            .complete_external("run-0", jevia_core::Outcome::Success, None, true)
            .await
            .is_err()
    );
    assert_eq!(f.storage.get("run-0").await.unwrap(), pending);
}

async fn contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    assert_eq!(f.storage.check_deep().await.unwrap(), 0);
    assert_eq!(f.counter().await, 0);
    f.seed().await;
    let before = f.storage.recent(usize::MAX, false).await.unwrap();
    // An invalid record in a different configured project must not affect this scan.
    let other = Fixture::new(postgres).await;
    other.storage.append(&record(0)).await.unwrap();
    sqlx::query("UPDATE jevia_runs SET record = '{private-invalid' WHERE project = $1")
        .bind(&other.db().project)
        .execute(&other.db().pool)
        .await
        .unwrap();
    assert!(other.storage.check_deep().await.is_err());
    assert_eq!(f.storage.check_deep().await.unwrap(), RECORDS);
    assert_eq!(f.storage.recent(usize::MAX, false).await.unwrap(), before);
    assert_eq!(f.counter().await, RECORDS as i64);

    // A held writer lock does not block the diagnostic or insert a CRUD probe.
    let mut writer = f.db().write().await.unwrap();
    sqlx::query("UPDATE jevia_runs SET learning = 2 WHERE project = $1 AND run_id = 'run-404'")
        .bind(&f.db().project)
        .execute(&mut *writer)
        .await
        .unwrap();
    // This asserts lock independence, not throughput. The writer remains held
    // until the read succeeds, so allowing a loaded CI runner more time cannot
    // hide a diagnostic that waits for that writer to release its lock.
    let checked = tokio::time::timeout(std::time::Duration::from_secs(30), f.storage.check_deep())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(checked, RECORDS);
    writer.rollback().await.unwrap();
    assert_eq!(f.counter().await, RECORDS as i64);
}

async fn corruption(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed().await;
    let mut unsupported = record(404);
    unsupported.schema_version = 999;
    let mut wrong_id = record(404);
    wrong_id.decision.run_id = "private-wrong-id".into();
    let mut empty_id = record(404);
    empty_id.decision.run_id.clear();
    let mut bad_probability = record(404);
    bad_probability
        .decision
        .probabilities
        .insert("private-tier".into(), 2.0);
    let mut blank_model = record(404);
    blank_model.decision.jev_model = " \t".into();
    let valid = serde_json::to_string(&record(404)).unwrap();
    for invalid in [
        "{private-invalid".into(),
        serde_json::to_string(&unsupported).unwrap(),
        serde_json::to_string(&wrong_id).unwrap(),
        serde_json::to_string(&empty_id).unwrap(),
        serde_json::to_string(&bad_probability).unwrap(),
        serde_json::to_string(&blank_model).unwrap(),
    ] {
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'run-404'")
            .bind(&f.db().project)
            .bind(&invalid)
            .execute(&f.db().pool)
            .await
            .unwrap();
        // The original access check intentionally remains fast and does not parse records.
        assert_eq!(f.storage.check().await.unwrap(), RECORDS);
        let error = f.storage.check_deep().await.unwrap_err();
        assert!(format!("{error:#}").contains("405")); // Scan beyond two pages.
        assert!(!format!("{error:#}").contains("private-"));
        let stored: String = sqlx::query_scalar(
            "SELECT record FROM jevia_runs WHERE project = $1 AND run_id = 'run-404'",
        )
        .bind(&f.db().project)
        .fetch_one(&f.db().pool)
        .await
        .unwrap();
        assert_eq!(stored, invalid);
    }
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'run-404'")
        .bind(&f.db().project)
        .bind(valid)
        .execute(&f.db().pool)
        .await
        .unwrap();
    for learning in [0_i64, 2] {
        sqlx::query(
            "UPDATE jevia_runs SET learning = $2 WHERE project = $1 AND run_id = 'run-404'",
        )
        .bind(&f.db().project)
        .bind(learning)
        .execute(&f.db().pool)
        .await
        .unwrap();
        let error = f.storage.check_deep().await.unwrap_err();
        assert!(format!("{error:#}").contains("learning index mismatch"));
        let stored: i64 = sqlx::query_scalar(
            "SELECT learning FROM jevia_runs WHERE project = $1 AND run_id = 'run-404'",
        )
        .bind(&f.db().project)
        .fetch_one(&f.db().pool)
        .await
        .unwrap();
        assert_eq!(stored, learning);
    }
    sqlx::query("UPDATE jevia_runs SET learning = 1 WHERE project = $1 AND run_id = 'run-404'")
        .bind(&f.db().project)
        .execute(&f.db().pool)
        .await
        .unwrap();
    for ordinal in [-1_i64, 0, RECORDS as i64 + 1] {
        sqlx::query("UPDATE jevia_runs SET ordinal = $2 WHERE project = $1 AND run_id = 'run-404'")
            .bind(&f.db().project)
            .bind(ordinal)
            .execute(&f.db().pool)
            .await
            .unwrap();
        assert!(
            format!("{:#}", f.storage.check_deep().await.unwrap_err()).contains("append ordering")
        );
    }
    sqlx::query("UPDATE jevia_runs SET ordinal = $2 WHERE project = $1 AND run_id = 'run-404'")
        .bind(&f.db().project)
        .bind(RECORDS as i64)
        .execute(&f.db().pool)
        .await
        .unwrap();
    for next in [-1_i64, 1] {
        sqlx::query("UPDATE jevia_projects SET next_seq = $2 WHERE project = $1")
            .bind(&f.db().project)
            .bind(next)
            .execute(&f.db().pool)
            .await
            .unwrap();
        assert!(f.storage.check_deep().await.is_err());
        assert_eq!(f.counter().await, next);
    }
    sqlx::query("UPDATE jevia_projects SET next_seq = $2 WHERE project = $1")
        .bind(&f.db().project)
        .bind(RECORDS as i64)
        .execute(&f.db().pool)
        .await
        .unwrap();
    // Retention can leave gaps; the counter need not equal the largest retained ordinal.
    sqlx::query("DELETE FROM jevia_runs WHERE project = $1 AND run_id IN ('run-202', 'run-404')")
        .bind(&f.db().project)
        .execute(&f.db().pool)
        .await
        .unwrap();
    assert_eq!(f.storage.check_deep().await.unwrap(), RECORDS - 2);
    if !postgres {
        f.db().set_schema_version_for_test(999).await;
        assert!(f.storage.check_deep().await.is_err());
    }
}

async fn snapshot(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed().await;
    let mut tx = f.db().read_snapshot().await.unwrap();
    // Establish the same snapshot before another connection changes its indexes.
    let _: i64 = sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = $1")
        .bind(&f.db().project)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    f.storage.append(&record(RECORDS)).await.unwrap();
    sqlx::query("UPDATE jevia_runs SET learning = 2 WHERE project = $1 AND run_id = 'run-404'")
        .bind(&f.db().project)
        .execute(&f.db().pool)
        .await
        .unwrap();
    assert_eq!(f.db().inspect_snapshot(&mut tx).await.unwrap(), RECORDS);
    tx.rollback().await.unwrap();
    assert!(f.storage.check_deep().await.is_err()); // A new scan sees the committed corruption.
}

#[tokio::test]
async fn sqlite_deep_check_is_non_mutating_and_project_scoped() {
    contract(false).await;
}
#[tokio::test]
async fn sqlite_deep_check_reports_corrupt_records_and_indexes_without_repair() {
    corruption(false).await;
}
#[tokio::test]
async fn sqlite_deep_check_uses_one_consistent_snapshot() {
    snapshot(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_deep_check_is_non_mutating_and_project_scoped() {
    contract(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_deep_check_reports_corrupt_records_and_indexes_without_repair() {
    corruption(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_deep_check_uses_one_consistent_snapshot() {
    snapshot(true).await;
}
