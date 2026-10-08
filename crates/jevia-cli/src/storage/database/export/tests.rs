use super::*;
use crate::{paths::ProjectPaths, storage::Storage};
use jevia_core::{Config, Outcome, StorageConfig};
use serde_json::json;
use std::{fs, io};

const RECORDS: usize = PAGE_SIZE as usize * 2 + 5;

struct Fixture {
    dir: tempfile::TempDir,
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
                    project: format!("export-{}", uuid::Uuid::new_v4()),
                    allow_insecure_localhost: true,
                }
            } else {
                StorageConfig::Sqlite {
                    url: "sqlite://.jevia/export.db".into(),
                }
            },
            ..Config::default()
        };
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        Self { dir, storage }
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
        // Keyset paging must not skip negative/zero ordinals at the first page.
        sqlx::query(
            "UPDATE jevia_runs SET ordinal = -2 WHERE project = $1 AND run_id = 'export-0'",
        )
        .bind(&self.db().project)
        .execute(&self.db().pool)
        .await
        .unwrap();
    }
}

fn record(index: usize) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 3, "run_id": format!("export-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": RECORDS.saturating_sub(index), "task": "private-export-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()
}

fn decode_file(path: &std::path::Path) -> Vec<RouteRecord> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let empty = f.dir.path().join("empty.jsonl");
    assert_eq!(f.storage.export(&empty).await.unwrap(), 0);
    assert!(fs::read(&empty).unwrap().is_empty());
    f.seed().await;
    let other = Fixture::new(postgres).await;
    let mut different = record(0);
    different.task = Some("different project".into());
    other.storage.append(&different).await.unwrap();
    let output = f.dir.path().join("snapshot.jsonl");
    assert_eq!(f.storage.export(&output).await.unwrap(), RECORDS);
    assert_eq!(
        decode_file(&output),
        (0..RECORDS).map(record).collect::<Vec<_>>()
    );
    assert_eq!(other.storage.get("export-0").await.unwrap(), different);
    let original = fs::read(&output).unwrap();
    assert!(f.storage.export(&output).await.is_err());
    assert_eq!(fs::read(&output).unwrap(), original);
    assert!(
        f.storage
            .export(&f.dir.path().join("missing/snapshot.jsonl"))
            .await
            .is_err()
    );
    assert_eq!(f.storage.check().await.unwrap(), RECORDS);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(output).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

async fn consistent_snapshot(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed().await;
    let mut tx = f.db().read_snapshot().await.unwrap();
    let mut exported = Vec::new();
    let (mut cursor, count) = f
        .db()
        .visit_export_page(&mut tx, None, |record| {
            exported.push(record.clone());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(count, PAGE_SIZE as usize);
    // Mutate rows not yet exported on a different connection. All these commits
    // finish while the reader is open: export must not take a project write lock.
    let mut writer = f.db().write().await.unwrap();
    let mut changed = record(205);
    changed.task = Some("changed after snapshot".into());
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'export-205'")
        .bind(&f.db().project)
        .bind(serde_json::to_string(&changed).unwrap())
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("DELETE FROM jevia_runs WHERE project = $1 AND run_id = 'export-206'")
        .bind(&f.db().project)
        .execute(&mut *writer)
        .await
        .unwrap();
    f.db().insert(&mut writer, &record(RECORDS)).await.unwrap();
    writer.commit().await.unwrap();
    loop {
        let (last, count) = f
            .db()
            .visit_export_page(&mut tx, cursor, |record| {
                exported.push(record.clone());
                Ok(())
            })
            .await
            .unwrap();
        if count == 0 {
            break;
        }
        assert!(count <= PAGE_SIZE as usize);
        cursor = last;
    }
    tx.rollback().await.unwrap();
    assert_eq!(exported, (0..RECORDS).map(record).collect::<Vec<_>>());
    let output = f.dir.path().join("after.jsonl");
    f.storage.export(&output).await.unwrap();
    let after = decode_file(&output);
    assert_eq!(after.len(), RECORDS);
    assert_eq!(after[205], changed);
    assert!(
        !after
            .iter()
            .any(|record| record.decision.run_id == "export-206")
    );
    assert_eq!(after.last().unwrap(), &record(RECORDS));
    if postgres {
        let mut read_only = f.db().read_snapshot().await.unwrap();
        assert!(
            sqlx::query("UPDATE jevia_projects SET next_seq = next_seq WHERE project = $1")
                .bind(&f.db().project)
                .execute(&mut *read_only)
                .await
                .is_err()
        );
        read_only.rollback().await.unwrap();
    }
}

struct FailingWriter {
    remaining: usize,
}
impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("simulated full disk"));
        }
        let count = bytes.len().min(self.remaining);
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn failures(postgres: bool) {
    let f = Fixture::new(postgres).await;
    f.seed().await;
    // Abort after a page has already been written, without leaving a final file.
    let mut unsupported = record(402);
    unsupported.schema_version = 999;
    let mut mismatched = record(402);
    mismatched.decision.run_id = "wrong-id".into();
    for invalid in [
        "{private-malformed".into(),
        serde_json::to_string(&unsupported).unwrap(),
        serde_json::to_string(&mismatched).unwrap(),
    ] {
        sqlx::query(
            "UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'export-402'",
        )
        .bind(&f.db().project)
        .bind(invalid)
        .execute(&f.db().pool)
        .await
        .unwrap();
        let output = f.dir.path().join("failed.jsonl");
        let error = f.storage.export(&output).await.unwrap_err();
        assert!(!format!("{error:#}").contains("private-"));
        assert!(!output.exists());
        assert!(!fs::read_dir(f.dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".jevia-export-")
        }));
    }
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'export-402'")
        .bind(&f.db().project)
        .bind(serde_json::to_string(&record(402)).unwrap())
        .execute(&f.db().pool)
        .await
        .unwrap();
    assert!(
        f.db()
            .export(FailingWriter { remaining: 180_000 })
            .await
            .is_err()
    );
    assert_eq!(
        f.storage.get("export-402").await.unwrap().outcome,
        Outcome::Success
    );
    // Failed readers release their transaction; later writes/exports still work.
    f.storage.append(&record(RECORDS)).await.unwrap();
    assert_eq!(
        f.storage
            .export(&f.dir.path().join("retry.jsonl"))
            .await
            .unwrap(),
        RECORDS + 1
    );

    // A failed write of the first row must stop before decoding the second.
    // This also prevents a regression to collecting decoded pages in memory.
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'export-1'")
        .bind(&f.db().project)
        .bind("{private-malformed")
        .execute(&f.db().pool)
        .await
        .unwrap();
    let error = f
        .db()
        .export(FailingWriter { remaining: 0 })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "simulated full disk");
    let mut tx = f.db().write().await.unwrap();
    f.db().insert(&mut tx, &record(RECORDS + 1)).await.unwrap();
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn sqlite_export_bounded_contract() {
    contract(false).await;
}

async fn recovery_size_limit(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let mut boundary = record(0);
    boundary.task = Some(String::new());
    let overhead = crate::jsonl::encode(&boundary).unwrap().len();
    boundary.task = Some("x".repeat(crate::jsonl::MAX_LINE_BYTES - overhead));
    assert_eq!(
        crate::jsonl::encode(&boundary).unwrap().len(),
        crate::jsonl::MAX_LINE_BYTES
    );
    f.storage.append(&boundary).await.unwrap();
    let output = f.dir.path().join("boundary.jsonl");
    f.storage.export(&output).await.unwrap();
    let lines = crate::jsonl::lines(io::BufReader::new(fs::File::open(output).unwrap()));
    assert_eq!(lines.map(Result::unwrap).count(), 1);

    // Feedback cannot silently create a row whose next backup is unrestorable.
    assert!(
        f.db()
            .outcome("export-0", Outcome::Failure, Some("new feedback"))
            .await
            .is_err()
    );
    assert_eq!(f.storage.get("export-0").await.unwrap(), boundary);
    let mut oversized = boundary.clone();
    oversized.task.as_mut().unwrap().push('x');
    let mut tx = f.db().write().await.unwrap();
    assert!(f.db().insert(&mut tx, &oversized).await.is_err());
    let counter: i64 = sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = $1")
        .bind(&f.db().project)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(counter, 1);
    tx.rollback().await.unwrap();

    // Simulate an oversized row written by an older client. Reads still work,
    // but export and both archive modes fail without publishing or deleting.
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'export-0'")
        .bind(&f.db().project)
        .bind(serde_json::to_string(&oversized).unwrap())
        .execute(&f.db().pool)
        .await
        .unwrap();
    f.storage.append(&record(1)).await.unwrap();
    let output = f.dir.path().join("oversized.jsonl");
    assert!(f.storage.export(&output).await.is_err());
    assert!(!output.exists());
    for apply in [false, true] {
        let error = f.db().archive(f.dir.path(), 1, apply).await.unwrap_err();
        assert!(error.to_string().contains("8 MiB"));
        assert!(!error.to_string().contains("private-"));
        assert_eq!(f.storage.get("export-0").await.unwrap(), oversized);
        assert!(f.storage.get("export-1").await.is_ok());
        assert!(!f.dir.path().join("history-backups").exists());
        assert!(!f.dir.path().join("history-archives").exists());
    }
}

#[tokio::test]
async fn sqlite_recovery_size_limit() {
    recovery_size_limit(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_recovery_size_limit() {
    recovery_size_limit(true).await;
}
#[tokio::test]
async fn sqlite_export_consistent_snapshot_allows_writers() {
    consistent_snapshot(false).await;
}
#[tokio::test]
async fn sqlite_export_failure_never_publishes_partial_file() {
    failures(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_export_bounded_contract() {
    contract(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_export_consistent_snapshot_allows_writers() {
    consistent_snapshot(true).await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_export_failure_never_publishes_partial_file() {
    failures(true).await;
}
