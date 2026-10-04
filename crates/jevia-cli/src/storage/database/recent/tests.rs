use super::*;
use crate::storage::Storage;
use jevia_core::Config;

const RECORDS: usize = PAGE_SIZE * 2 + 5;

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
                    project: format!("stats-window-{}", uuid::Uuid::new_v4()),
                    allow_insecure_localhost: true,
                }
            } else {
                StorageConfig::Sqlite {
                    url: "sqlite://.jevia/stats.db".into(),
                }
            },
            ..Config::default()
        };
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        let f = Self { _dir: dir, storage };
        let mut tx = f.db().write().await.unwrap();
        for i in 0..RECORDS {
            f.db().insert(&mut tx, &record(i)).await.unwrap();
        }
        // Exercise ordinal gaps, zero, and the signed endpoints. No arithmetic
        // sentinel should accidentally exclude a valid first/last row.
        sqlx::query("UPDATE jevia_runs SET ordinal = -ordinal * 2 WHERE project = $1")
            .bind(&f.db().project)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE jevia_runs SET ordinal = -ordinal - 2 WHERE project = $1")
            .bind(&f.db().project)
            .execute(&mut *tx)
            .await
            .unwrap();
        for (id, ordinal) in [
            ("stats-0", i64::MIN),
            ("stats-1", 0),
            ("stats-404", i64::MAX),
        ] {
            sqlx::query("UPDATE jevia_runs SET ordinal = $2 WHERE project = $1 AND run_id = $3")
                .bind(&f.db().project)
                .bind(ordinal)
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE jevia_projects SET next_seq = 10000 WHERE project = $1")
            .bind(&f.db().project)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        f
    }

    fn db(&self) -> &Database {
        let Storage::Database(db) = &self.storage else {
            unreachable!()
        };
        db
    }
}

fn record(index: usize) -> RouteRecord {
    let mut record = crate::tests::sample_record();
    record.decision.run_id = format!("stats-{index}");
    record.decision.created_at_ms = RECORDS.saturating_sub(index) as u64;
    record
}

async fn window_contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    // A second project on PostgreSQL must not enter this window.
    let other = Fixture::new(postgres).await;
    other.storage.append(&record(RECORDS)).await.unwrap();
    for limit in [
        0,
        1,
        PAGE_SIZE,
        PAGE_SIZE + 1,
        RECORDS,
        RECORDS + 1,
        100_000,
    ] {
        let mut seen = Vec::new();
        let older = f
            .storage
            .visit_recent(limit, |entry| {
                seen.push(entry.clone());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(older, RECORDS > limit);
        assert_eq!(
            seen,
            (RECORDS.saturating_sub(limit)..RECORDS)
                .map(record)
                .collect::<Vec<_>>()
        );
    }
    assert!(
        f.storage
            .visit_recent(300, |_| bail!("callback failed"))
            .await
            .is_err()
    );
    // A failed visitor must release the snapshot/connection for the next read.
    assert!(f.storage.visit_recent(1, |_| Ok(())).await.unwrap());

    // Preserve old behavior: selected rows AND the limit+1 sentinel validate;
    // older unrelated SQL rows are not decoded.
    for (index, should_fail) in [
        (0, false),
        (RECORDS - 3, false),
        (RECORDS - 2, true),
        (RECORDS - 1, true),
    ] {
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = $3")
            .bind(&f.db().project)
            .bind("{PRIVATE malformed}")
            .bind(format!("stats-{index}"))
            .execute(&f.db().pool)
            .await
            .unwrap();
        let result = f.storage.visit_recent(1, |_| Ok(())).await;
        assert_eq!(result.is_err(), should_fail);
        if let Err(error) = result {
            assert!(!format!("{error:#}").contains("PRIVATE"));
        }
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = $3")
            .bind(&f.db().project)
            .bind(serde_json::to_string(&record(index)).unwrap())
            .bind(format!("stats-{index}"))
            .execute(&f.db().pool)
            .await
            .unwrap();
    }
}

async fn consistent_snapshot(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let mut tx = f.db().read_snapshot().await.unwrap();
    let limit = 401;
    let mut cursor = f.db().recent_boundary(&mut tx, limit).await.unwrap();
    assert!(cursor.is_some());
    // Change the boundary and unseen rows on a separate connection. The reader
    // must not block writes or combine its boundary with a newer row snapshot.
    let mut writer = f.db().write().await.unwrap();
    sqlx::query("DELETE FROM jevia_runs WHERE project = $1 AND run_id IN ('stats-3', 'stats-203', 'stats-204')")
        .bind(&f.db().project).execute(&mut *writer).await.unwrap();
    let mut changed = record(300);
    changed.task = Some("changed after snapshot".into());
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1 AND run_id = 'stats-300'")
        .bind(&f.db().project)
        .bind(serde_json::to_string(&changed).unwrap())
        .execute(&mut *writer)
        .await
        .unwrap();
    f.db().insert(&mut writer, &record(RECORDS)).await.unwrap();
    writer.commit().await.unwrap();
    let mut seen = Vec::new();
    loop {
        let page = f
            .db()
            .recent_page(&mut tx, cursor, PAGE_SIZE)
            .await
            .unwrap();
        assert!(page.len() <= PAGE_SIZE);
        if page.is_empty() {
            break;
        }
        for (ordinal, raw) in page {
            seen.push(decode(&raw).unwrap());
            cursor = Some(ordinal);
        }
    }
    tx.rollback().await.unwrap();
    assert_eq!(
        seen,
        (RECORDS - limit..RECORDS).map(record).collect::<Vec<_>>()
    );
    assert_eq!(f.storage.get("stats-300").await.unwrap(), changed);
    assert!(f.storage.get("stats-203").await.is_err());
}

#[tokio::test]
async fn sqlite_recent_window_contract() {
    window_contract(false).await;
}

#[tokio::test]
async fn sqlite_recent_window_snapshot() {
    consistent_snapshot(false).await;
}

#[tokio::test]
#[ignore = "requires JEVIA_TEST_POSTGRES_URL"]
async fn postgres_recent_window_contract() {
    window_contract(true).await;
}

#[tokio::test]
#[ignore = "requires JEVIA_TEST_POSTGRES_URL"]
async fn postgres_recent_window_snapshot() {
    consistent_snapshot(true).await;
}
