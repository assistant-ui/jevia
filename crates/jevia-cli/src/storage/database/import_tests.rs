use super::*;
use crate::storage::Storage;
use jevia_core::Config;
use serde_json::json;

fn record(index: usize) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 3, "run_id": format!("import-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 405_usize.saturating_sub(index), "task": "private-import-task", "outcome": "success"
    })).unwrap()
}

async fn counter(database: &Database) -> i64 {
    sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = $1")
        .bind(&database.project)
        .fetch_one(&database.pool)
        .await
        .unwrap()
}

async fn contract(postgres: bool) {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(dir.path().into());
    let config = Config {
        storage: if postgres {
            StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("stream-import-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            }
        } else {
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/import.db".into(),
            }
        },
        ..Config::default()
    };
    let storage = Storage::open(&config, &paths, true).await.unwrap();
    let Storage::Database(database) = &storage else {
        unreachable!()
    };
    storage.append(&record(405)).await.unwrap();
    let before = counter(database).await;

    // Source validation must finish before competing for the SQL write lock.
    let source = dir.path().join("invalid.jsonl");
    fs::write(&source, "{private-invalid").unwrap();
    let held_writer = database.write().await.unwrap();
    for apply in [false, true] {
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            storage.import_jsonl(source.clone(), apply),
        )
        .await
        .expect("source validation tried to take the database write lock");
        assert!(format!("{:#}", result.unwrap_err()).contains("invalid import record"));
    }
    held_writer.rollback().await.unwrap();

    for apply in [false, true] {
        // Simulate an I/O error from the private snapshot after many inserts.
        let records = (0..405)
            .map(|index| Ok(record(index)))
            .chain(std::iter::once(Err(anyhow!("snapshot read failed"))));
        assert!(database.import(records, apply).await.is_err());
        assert_eq!(storage.check_deep().await.unwrap(), 1);
        assert_eq!(counter(database).await, before);

        let mut conflict = record(405);
        conflict.task = Some("private-conflicting-task".into());
        let records = (0..405)
            .map(|index| Ok(record(index)))
            .chain(std::iter::once(Ok(conflict)));
        let error = database.import(records, apply).await.unwrap_err();
        assert!(format!("{error:#}").contains("conflicts"));
        assert!(!format!("{error:#}").contains("private-"));
        assert_eq!(storage.check_deep().await.unwrap(), 1);
        assert_eq!(counter(database).await, before);
    }
    assert_eq!(
        database
            .import((0..406).map(|index| Ok(record(index))), false)
            .await
            .unwrap(),
        (405, 1)
    );
    assert_eq!(counter(database).await, before);
    assert_eq!(
        database
            .import((0..406).map(|index| Ok(record(index))), true)
            .await
            .unwrap(),
        (405, 1)
    );
    assert_eq!(storage.check_deep().await.unwrap(), 406);
    assert_eq!(counter(database).await, before + 405);
    let expected = std::iter::once(record(405))
        .chain((0..405).map(record))
        .collect::<Vec<_>>();
    assert_eq!(storage.recent(1000, false).await.unwrap(), expected);
}

#[tokio::test]
async fn sqlite_streaming_import_rolls_back_late_failures_and_sequence() {
    contract(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_streaming_import_rolls_back_late_failures_and_sequence() {
    contract(true).await;
}
