use super::*;
use crate::{paths::ProjectPaths, storage::Storage, store};
use jevia_core::{Config, Outcome, RouteRecord, StorageConfig};
use serde_json::{Value, json};

const TERMINAL: usize = PAGE_SIZE * 2 + 5;

struct Fixture {
    _dir: tempfile::TempDir,
    paths: ProjectPaths,
    config: Config,
    db: Database,
}

impl Fixture {
    async fn new(postgres: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(dir.path().to_owned());
        fs::create_dir(&paths.directory).unwrap();
        let config = Config {
            storage: if postgres {
                StorageConfig::Postgres {
                    url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                    project: format!("archive-{}", uuid::Uuid::new_v4()),
                    allow_insecure_localhost: true,
                }
            } else {
                StorageConfig::Sqlite {
                    url: "sqlite://.jevia/archive.db".into(),
                }
            },
            ..Config::default()
        };
        let Storage::Database(db) = Storage::open(&config, &paths, true).await.unwrap() else {
            panic!("expected SQL")
        };
        Self {
            _dir: dir,
            paths,
            config,
            db,
        }
    }

    async fn seed(&self, count: usize) {
        let mut tx = self.db.write().await.unwrap();
        for index in 0..count {
            let mut row = record(&format!("row-{index}"), Some("completed"), "success");
            row.decision.created_at_ms = (count - index) as u64;
            self.db.insert(&mut tx, &row).await.unwrap();
        }
        tx.commit().await.unwrap();
    }
}

fn record(id: &str, state: Option<&str>, outcome: &str) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 3, "run_id": id, "tier": "fast", "suggested_tier": "fast", "confidence": 0.9,
        "probabilities": {}, "fallback_applied": false, "jev_model": "test", "created_at_ms": 1,
        "task": "private-task", "outcome": outcome, "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": state.map(|state| json!({"state": state, "started_at_ms": 1, "finished_at_ms": 2})),
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": outcome, "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()
}

fn jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn eligibility_protects_lifecycle_and_execution_ownership() {
    for (state, expected) in [
        ("routed", false),
        ("running", false),
        ("verifying", false),
        ("completed", true),
        ("launch_failed", true),
        ("interrupted", true),
        ("cancelled", true),
        ("timed_out", true),
    ] {
        for outcome in ["unknown", "success", "failure"] {
            let mut row = ArchiveRow {
                id: "id".into(),
                ordinal: 1,
                owner: String::new(),
                raw: serde_json::to_string(&record("id", Some(state), outcome)).unwrap(),
            };
            assert_eq!(row.eligible().unwrap(), expected, "{state} / {outcome}");
            row.owner = "supervisor".into();
            assert!(!row.eligible().unwrap());
        }
    }
    for outcome in ["unknown", "success", "failure"] {
        let row = ArchiveRow {
            id: "id".into(),
            ordinal: 1,
            owner: String::new(),
            raw: serde_json::to_string(&record("id", None, outcome)).unwrap(),
        };
        assert_eq!(row.eligible().unwrap(), outcome != "unknown");
    }
}

async fn contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed(TERMINAL).await;
    f.db.append(&record("legacy-known", None, "success"))
        .await
        .unwrap();
    for (id, state, outcome) in [
        ("running", Some("running"), "unknown"),
        ("verifying", Some("verifying"), "unknown"),
        ("pending", Some("routed"), "success"),
        ("legacy-unknown", None, "unknown"),
        ("owned-terminal", Some("completed"), "success"),
    ] {
        f.db.append(&record(id, state, outcome)).await.unwrap();
    }
    sqlx::query(
        "UPDATE jevia_runs SET owner = 'claimed' WHERE project = $1 AND run_id = 'owned-terminal'",
    )
    .bind(&f.db.project)
    .execute(&f.db.pool)
    .await
    .unwrap();
    let mut extra = serde_json::to_value(f.db.get("row-0").await.unwrap()).unwrap();
    extra["extra"] = json!({"preserve": true, "text": "line\nbreak\rreturn"});
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'row-0'")
        .bind(&f.db.project)
        .bind(serde_json::to_string_pretty(&extra).unwrap())
        .execute(&f.db.pool)
        .await
        .unwrap();
    let other = Fixture::new(postgres).await;
    other
        .db
        .append(&record("row-0", Some("completed"), "failure"))
        .await
        .unwrap();
    let expected = TERMINAL + 1 - 3;
    let preview = f.db.archive(&f.paths.directory, 3, false).await.unwrap();
    assert_eq!(preview.archived_records, expected);
    assert_eq!(preview.retained_records, 8);
    assert!(preview.would_change && !preview.applied);
    assert!(preview.backup.is_none() && preview.archive.is_none());
    assert!(!f.paths.directory.join("history-backups").exists());
    assert!(!f.paths.directory.join("history-archives").exists());
    assert_eq!(
        f.db.recent(usize::MAX, false).await.unwrap().len(),
        TERMINAL + 6
    );
    let report = f.db.archive(&f.paths.directory, 3, true).await.unwrap();
    assert!(report.applied);
    assert_eq!(report.archived_records, expected);
    let backup = report.backup.unwrap();
    let archive = report.archive.unwrap();
    let saved = jsonl(&archive);
    assert_eq!(saved.len(), expected);
    assert_eq!(saved[0], extra);
    assert_eq!(
        saved.last().unwrap()["run_id"],
        format!("row-{}", expected - 1)
    );
    assert_eq!(jsonl(&backup).len(), TERMINAL + 6);
    assert_eq!(f.db.recent(usize::MAX, false).await.unwrap().len(), 8);
    for id in [
        "running",
        "verifying",
        "pending",
        "legacy-unknown",
        "owned-terminal",
        "legacy-known",
        "row-403",
        "row-404",
    ] {
        assert!(f.db.get(id).await.is_ok(), "{id} must be retained");
    }
    assert_eq!(
        other.db.get("row-0").await.unwrap().outcome,
        Outcome::Failure
    );
    assert_eq!(other.db.recent(usize::MAX, false).await.unwrap().len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&archive, &backup] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    let no_op = f.db.archive(&f.paths.directory, 3, true).await.unwrap();
    assert!(!no_op.applied && !no_op.would_change);
    assert_eq!(fs::read_dir(archive.parent().unwrap()).unwrap().count(), 1);
    assert!(f.db.archive(&f.paths.directory, 0, true).await.is_err());
    let imported = store::load(&archive).unwrap();
    assert_eq!(f.db.import(&imported, false).await.unwrap(), (expected, 0));
    assert_eq!(f.db.import(&imported, true).await.unwrap(), (expected, 0));
    assert_eq!(f.db.import(&imported, true).await.unwrap(), (0, expected));
    assert_eq!(f.db.get("row-0").await.unwrap(), imported[0]);
    assert_eq!(jsonl(&archive), saved); // Import may add a JSONL lock sidecar, not rewrite the archive.
    let mut tx = f.db.write().await.unwrap();
    let next: i64 = sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = $1")
        .bind(&f.db.project)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(next, (TERMINAL + 6 + expected) as i64); // Never reset append order during retention.
}

async fn failures(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed(TERMINAL).await;
    // Disk failure before finalizing either recovery file must leave every row.
    fs::write(
        f.paths.directory.join("history-archives"),
        b"not a directory",
    )
    .unwrap();
    assert!(f.db.archive(&f.paths.directory, 1, true).await.is_err());
    assert_eq!(
        f.db.recent(usize::MAX, false).await.unwrap().len(),
        TERMINAL
    );
    fs::remove_file(f.paths.directory.join("history-archives")).unwrap();
    // Force a failure in the second DELETE batch, after the first batch succeeded.
    let trigger = format!("archive_refuse_{}", uuid::Uuid::new_v4().simple());
    if postgres {
        sqlx::query(&format!("CREATE FUNCTION {trigger}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF OLD.project = '{}' AND OLD.run_id = 'row-205' THEN RAISE EXCEPTION 'private-trigger-error'; END IF; RETURN OLD; END $$", f.db.project))
            .execute(&f.db.pool).await.unwrap();
        sqlx::query(&format!("CREATE TRIGGER {trigger} BEFORE DELETE ON jevia_runs FOR EACH ROW EXECUTE FUNCTION {trigger}()"))
            .execute(&f.db.pool).await.unwrap();
    } else {
        sqlx::query(&format!("CREATE TRIGGER {trigger} BEFORE DELETE ON jevia_runs WHEN OLD.run_id = 'row-205' BEGIN SELECT RAISE(ABORT, 'private-trigger-error'); END"))
            .execute(&f.db.pool).await.unwrap();
    }
    let error = f.db.archive(&f.paths.directory, 1, true).await.unwrap_err();
    assert!(!format!("{error:#}").contains("private-trigger-error"));
    assert!(format!("{error:#}").contains("Backup:"));
    assert_eq!(
        f.db.recent(usize::MAX, false).await.unwrap().len(),
        TERMINAL
    );
    let backups: Vec<_> = fs::read_dir(f.paths.directory.join("history-backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let archives: Vec<_> = fs::read_dir(f.paths.directory.join("history-archives"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(archives.len(), 1);
    assert_eq!(jsonl(&backups[0]).len(), TERMINAL);
    assert_eq!(jsonl(&archives[0]).len(), TERMINAL - 1);
    if postgres {
        sqlx::query(&format!("DROP TRIGGER {trigger} ON jevia_runs"))
            .execute(&f.db.pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP FUNCTION {trigger}()"))
            .execute(&f.db.pool)
            .await
            .unwrap();
    } else {
        sqlx::query(&format!("DROP TRIGGER {trigger}"))
            .execute(&f.db.pool)
            .await
            .unwrap();
    }
    f.db.archive(&f.paths.directory, 1, true).await.unwrap();
    assert_eq!(f.db.recent(usize::MAX, false).await.unwrap().len(), 1);
    assert_eq!(
        fs::read_dir(f.paths.directory.join("history-archives"))
            .unwrap()
            .count(),
        2
    );
    assert_eq!(jsonl(&archives[0]).len(), TERMINAL - 1); // Previous archive not overwritten.
}

async fn invalid_and_concurrent(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let empty = f.db.archive(&f.paths.directory, 1, true).await.unwrap();
    assert!(!empty.applied && !empty.would_change);
    assert_eq!(empty.retained_records, 0);
    assert!(!f.paths.directory.join("history-backups").exists());
    f.seed(3).await;
    let mut unsupported = record("row-2", Some("completed"), "success");
    unsupported.schema_version = 999;
    for raw in [
        "{private-invalid".into(),
        serde_json::to_string(&unsupported).unwrap(),
        serde_json::to_string(&record("wrong-id", Some("completed"), "success")).unwrap(),
    ] {
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'row-2'")
            .bind(&f.db.project)
            .bind(raw)
            .execute(&f.db.pool)
            .await
            .unwrap();
        let error = f.db.archive(&f.paths.directory, 1, true).await.unwrap_err();
        assert!(!format!("{error:#}").contains("private-"));
        assert!(!f.paths.directory.join("history-backups").exists());
    }
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'row-2'")
        .bind(&f.db.project)
        .bind(serde_json::to_string(&record("row-2", Some("completed"), "success")).unwrap())
        .execute(&f.db.pool)
        .await
        .unwrap();
    // Conditional deletion must reject data newer than the saved snapshot.
    let mut tx = f.db.write().await.unwrap();
    let mut rows = f.db.archive_page(&mut tx, None).await.unwrap();
    rows[0].raw = serde_json::to_string(&record("row-0", Some("completed"), "failure")).unwrap();
    assert!(f.db.delete_batch(&mut tx, &rows[..1]).await.is_err());
    tx.rollback().await.unwrap();
    // An in-flight writer completes before archival plans candidates.
    let second = Database::open(&f.config.storage, &f.paths, false)
        .await
        .unwrap();
    let mut tx = f.db.write().await.unwrap();
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'row-0'")
        .bind(&f.db.project)
        .bind(serde_json::to_string(&record("row-0", Some("running"), "unknown")).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    let directory = f.paths.directory.clone();
    let task = tokio::spawn(async move { second.archive(&directory, 1, true).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!task.is_finished());
    tx.commit().await.unwrap();
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.archived_records, 1);
    assert!(f.db.get("row-0").await.is_ok());
    assert!(f.db.get("row-2").await.is_ok());
    assert!(f.db.get("row-1").await.is_err());
}

#[tokio::test]
async fn sqlite_archive_contract() {
    contract(false).await;
}
#[tokio::test]
async fn sqlite_archive_rolls_back_after_disk_or_partial_delete_failure() {
    failures(false).await;
}
#[tokio::test]
async fn sqlite_archive_rejects_invalid_rows_and_serializes_writers() {
    invalid_and_concurrent(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_archive_contract() {
    contract(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_archive_rolls_back_after_disk_or_partial_delete_failure() {
    failures(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_archive_rejects_invalid_rows_and_serializes_writers() {
    invalid_and_concurrent(true).await;
}
