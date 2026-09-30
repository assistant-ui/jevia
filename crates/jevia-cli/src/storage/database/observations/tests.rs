use super::*;
use serde_json::json;

struct Fixture {
    dir: tempfile::TempDir,
    config: StorageConfig,
    database: Database,
}

impl Fixture {
    async fn new(postgres: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = if postgres {
            StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("observation-index-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            }
        } else {
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/index.db".into(),
            }
        };
        let database = Database::open(&config, &ProjectPaths::at(dir.path().into()), true)
            .await
            .unwrap();
        Self {
            dir,
            config,
            database,
        }
    }
}

fn record(id: usize) -> RouteRecord {
    serde_json::from_value(json!({
        "schema_version": 6, "run_id": format!("observation-{id}"),
        "tier": "fast", "suggested_tier": "fast", "confidence": 0.9,
        "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        // Deliberately not append order. NUL and a literal escape must round-trip.
        "created_at_ms": 100_000_usize.saturating_sub(id),
        "task": "private\u{0}task \\u0000 🦀", "outcome": "unknown",
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "execution": {"harness": "test", "model": "model\u{0}", "duration_ms": 1, "exit_code": 0}
    }))
    .unwrap()
}

async fn eligible(database: &Database, limit: usize) -> Vec<RouteRecord> {
    let result = database.recent_observations(limit).await.unwrap();
    assert_eq!(
        result,
        database.legacy_recent_observations(limit).await.unwrap()
    );
    result
}

async fn predicate_contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let database = &f.database;
    let mut expected = Vec::new();
    let mut tx = database.write().await.unwrap();
    let mut id = 0;
    // Exhaustive parity with the Rust policy, including legacy schemas and
    // absent/explicit-null optional fields. No success inference from exits.
    for schema in 1..=RECORD_SCHEMA_VERSION {
        for state in [
            None,
            Some("routed"),
            Some("running"),
            Some("verifying"),
            Some("completed"),
            Some("launch_failed"),
            Some("interrupted"),
            Some("cancelled"),
            Some("timed_out"),
        ] {
            for execution in [false, true] {
                for outcome in ["unknown", "success", "failure"] {
                    for source in [
                        None,
                        Some("manual"),
                        Some("verification"),
                        Some("process_exit"),
                    ] {
                        let mut value = serde_json::to_value(record(id)).unwrap();
                        value["schema_version"] = json!(schema);
                        value["lifecycle"] =
                            state.map_or(serde_json::Value::Null, |state| json!({"state": state}));
                        if !execution {
                            value["execution"] = serde_json::Value::Null;
                        }
                        value["outcome"] = json!(outcome);
                        value["outcome_evidence"] = source.map_or(
                            serde_json::Value::Null,
                            |source| json!({"source": source, "recorded_at_ms": 2}),
                        );
                        let record: RouteRecord = serde_json::from_value(value.clone()).unwrap();
                        if record.is_execution_observation() && !record.is_learning_evidence() {
                            expected.push(record.clone());
                        }
                        database.insert(&mut tx, &record).await.unwrap();
                        if id.is_multiple_of(2) {
                            // Simulate an older writer: only existing columns,
                            // retaining explicit nulls rather than serde omissions.
                            sqlx::query("UPDATE jevia_runs SET record = $3 WHERE project = $1 AND run_id = $2")
                                .bind(&database.project).bind(&record.decision.run_id).bind(value.to_string())
                                .execute(&mut *tx).await.unwrap();
                        }
                        id += 1;
                    }
                }
            }
        }
    }
    tx.commit().await.unwrap();
    for limit in [0, 1, 20, 129, usize::MAX] {
        let offset = expected.len().saturating_sub(limit);
        assert_eq!(eligible(database, limit).await, expected[offset..]);
    }
    assert_eq!(database.check_deep().await.unwrap(), id);
}

async fn mutations_contract(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let database = &f.database;
    database.append(&record(0)).await.unwrap();
    database.append(&record(1)).await.unwrap();
    assert_eq!(eligible(database, 1).await, vec![record(1)]);
    // Feedback moves a run into/out of the separate known-outcome window.
    database
        .outcome("observation-1", Outcome::Success, None)
        .await
        .unwrap();
    assert_eq!(eligible(database, 1).await, vec![record(0)]);
    database
        .outcome("observation-1", Outcome::Unknown, Some("uncertain"))
        .await
        .unwrap();
    assert_eq!(
        eligible(database, 1).await[0].decision.run_id,
        "observation-1"
    );

    // Atomic import/rollback and retention automatically maintain the index.
    database
        .import([Ok(record(2)), Err(anyhow!("failed snapshot"))], true)
        .await
        .unwrap_err();
    assert_eq!(eligible(database, 10).await.len(), 2);
    database.import([Ok(record(2))], false).await.unwrap();
    assert_eq!(eligible(database, 10).await.len(), 2);
    database.import([Ok(record(2))], true).await.unwrap();
    assert_eq!(eligible(database, 1).await, vec![record(2)]);
    database.archive(f.dir.path(), 1, false).await.unwrap();
    assert_eq!(eligible(database, 10).await.len(), 3);
    database.archive(f.dir.path(), 1, true).await.unwrap();
    assert_eq!(eligible(database, 10).await, vec![record(2)]);

    // Raw old-writer updates need no new derived flag or version-dependent code.
    let mut active = record(2);
    active.lifecycle.as_mut().unwrap().state = RunState::Running;
    let mut tx = database.write().await.unwrap();
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1")
        .bind(&database.project)
        .bind(serde_json::to_string(&active).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(eligible(database, 10).await, vec![record(2)]);
    sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1")
        .bind(&database.project)
        .bind(serde_json::to_string(&active).unwrap())
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(eligible(database, 10).await.is_empty());

    // Corrupt candidates must still fail closed, with no contents in errors.
    let mut unsupported = record(2);
    unsupported.schema_version = 999;
    for invalid in [
        "{private-invalid".to_owned(),
        serde_json::to_string(&unsupported).unwrap(),
    ] {
        sqlx::query("UPDATE jevia_runs SET record = $2 WHERE project = $1")
            .bind(&database.project)
            .bind(invalid)
            .execute(&database.pool)
            .await
            .unwrap();
        let error = database.recent_observations(10).await.unwrap_err();
        assert!(!format!("{error:#}").contains("private-"));
        assert!(database.check_deep().await.is_err());
    }
}

async fn query_plan(postgres: bool) {
    let f = Fixture::new(postgres).await;
    let database = &f.database;
    if !database.supports_observation_index {
        return;
    } // PostgreSQL <16 fallback.
    database.append(&record(0)).await.unwrap();
    let mut tx = database.write().await.unwrap();
    for id in 1..=3000 {
        let mut pending = record(id);
        pending.lifecycle.as_mut().unwrap().state = RunState::Routed;
        database.insert(&mut tx, &pending).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(eligible(database, 20).await, vec![record(0)]);
    let explain = if postgres {
        "EXPLAIN"
    } else {
        "EXPLAIN QUERY PLAN"
    };
    let plan = sqlx::query(&format!("{explain} {}", database.observation_query()))
        .bind(&database.project)
        .bind(20_i64)
        .fetch_all(&database.pool)
        .await
        .unwrap();
    let details = plan
        .iter()
        .map(|row| row.get::<String, _>(if postgres { 0 } else { 3 }))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        details.contains(INDEX),
        "observation index not used: {details}"
    );
    assert!(
        !details.contains("TEMP B-TREE"),
        "unexpected sort: {details}"
    );
}

#[tokio::test]
async fn sqlite_observation_index_matches_rust_policy() {
    predicate_contract(false).await;
}

#[tokio::test]
async fn sqlite_observation_index_tracks_mutations() {
    mutations_contract(false).await;
}

#[tokio::test]
async fn sqlite_sparse_observations_use_index() {
    query_plan(false).await;
}

#[tokio::test]
async fn sqlite_observation_index_upgrade_is_explicit_and_preserves_history() {
    let f = Fixture::new(false).await;
    f.database.append(&record(0)).await.unwrap();
    // A previous CLI's schema-1 store has no observation index.
    sqlx::query(&format!("DROP INDEX {INDEX}"))
        .execute(&f.database.pool)
        .await
        .unwrap();
    f.database.pool.close().await;
    let database = Database::open(&f.config, &ProjectPaths::at(f.dir.path().into()), false)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = $1")
            .bind(INDEX)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "normal open must not migrate");
    assert_eq!(eligible(&database, 10).await, vec![record(0)]);
    let before: (String, i64, i64, String) =
        sqlx::query_as("SELECT record, ordinal, learning, owner FROM jevia_runs")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    database.initialize().await.unwrap();
    database.initialize().await.unwrap();
    let after: (String, i64, i64, String) =
        sqlx::query_as("SELECT record, ordinal, learning, owner FROM jevia_runs")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    let version: i64 = sqlx::query_scalar("SELECT version FROM jevia_schema")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let next_seq: i64 = sqlx::query_scalar("SELECT next_seq FROM jevia_projects")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = $1")
            .bind(INDEX)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!((version, next_seq, count), (1, 1, 1));
    assert_eq!(eligible(&database, 10).await, vec![record(0)]);
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_observation_index_matches_rust_policy() {
    predicate_contract(true).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_observation_index_tracks_mutations() {
    mutations_contract(true).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_sparse_observations_use_index() {
    query_plan(true).await;
}

async fn shared_history_snapshot(postgres: bool) {
    let mut f = Fixture::new(postgres).await;
    f.database.append(&record(0)).await.unwrap();
    let supports_index = f.database.supports_observation_index;
    for indexed in [false, true] {
        if indexed && !supports_index {
            continue;
        }
        f.database.supports_observation_index = indexed;
        for (before, after) in [
            (Outcome::Unknown, Outcome::Success),
            (Outcome::Success, Outcome::Unknown),
        ] {
            f.database
                .outcome("observation-0", before, Some("snapshot fixture"))
                .await
                .unwrap();
            // Run the production window readers with the same snapshot used by
            // routing_history, committing feedback exactly between the reads.
            let mut tx = f.database.read_snapshot().await.unwrap();
            let known = f.database.recent_with(&mut *tx, 20, true).await.unwrap();
            f.database
                .outcome("observation-0", after, Some("concurrent feedback"))
                .await
                .unwrap();
            let passive = f.database.observations_in(&mut tx, 20).await.unwrap();
            tx.commit().await.unwrap();
            assert_eq!(
                known.len() + passive.len(),
                1,
                "attempt vanished or appeared twice"
            );
            assert_eq!(known.iter().chain(&passive).next().unwrap().outcome, before);
            let (known, passive) = f.database.routing_history(20).await.unwrap();
            assert_eq!(known.len() + passive.len(), 1);
            assert_eq!(known.iter().chain(&passive).next().unwrap().outcome, after);
        }
    }
}

#[tokio::test]
async fn sqlite_routing_windows_share_a_feedback_snapshot() {
    shared_history_snapshot(false).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_routing_windows_share_a_feedback_snapshot() {
    shared_history_snapshot(true).await;
}
