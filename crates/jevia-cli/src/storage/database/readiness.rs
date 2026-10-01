use super::{Database, db, observations::INDEX};
use anyhow::Result;

mod predicate;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationIndexStatus {
    Present,
    Missing,
    Unavailable,
    Unsupported,
    NotApplicable,
}

impl std::fmt::Display for ObservationIndexStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Present => "present (catalog shape/state/predicate checked; query-plan use is not guaranteed)",
            Self::Missing => "missing; routing still works but may scan history. Back up, then run `jevia storage init` with schema permissions during a quiet maintenance window",
            Self::Unavailable => "unavailable; the named index has an unexpected shape/state/predicate. Inspect it with database tooling; initialization does not replace existing indexes",
            Self::Unsupported => "unsupported on PostgreSQL <16; compatible paginated lookup is active",
            Self::NotApplicable => "not applicable (JSONL)",
        })
    }
}

impl Database {
    pub async fn observation_index_status(&self) -> Result<ObservationIndexStatus> {
        self.index_status_named(INDEX).await
    }

    async fn index_status_named(&self, name: &str) -> Result<ObservationIndexStatus> {
        use ObservationIndexStatus::*;
        if !self.supports_observation_index {
            return Ok(Unsupported);
        }
        let mut tx = self.read_snapshot().await?;
        let status = if self.is_postgres() {
            // Resolve both objects in the connection's search path, as routing
            // does. Never mistake another schema/table's same-named index for it.
            let valid: Option<(i64, String)> = db(sqlx::query_as(
                "SELECT CASE WHEN i.indisvalid AND i.indisready AND i.indislive
                    AND NOT i.indisunique AND i.indpred IS NOT NULL
                    AND i.indnkeyatts = 2 AND i.indnatts = 2
                    AND pg_get_indexdef(i.indexrelid, 1, true) = 'project'
                    AND pg_get_indexdef(i.indexrelid, 2, true) = 'ordinal'
                    THEN 1::bigint ELSE 0::bigint END,
                    COALESCE(pg_get_expr(i.indpred, i.indrelid, false), '')
                 FROM pg_index i WHERE i.indexrelid = to_regclass($1)
                    AND i.indrelid = to_regclass('jevia_runs')",
            )
            .bind(name)
            .fetch_optional(&mut *tx))
            .await?;
            match valid {
                None => Missing,
                Some((1, expression)) if predicate::postgres_matches(&expression) => Present,
                Some(_) => Unavailable,
            }
        } else {
            let metadata: Option<(i64, i64)> = db(sqlx::query_as(
                "SELECT \"unique\", partial FROM pragma_index_list('jevia_runs') WHERE name = $1",
            )
            .bind(name)
            .fetch_optional(&mut *tx))
            .await?;
            match metadata {
                None => Missing,
                Some((0, 1)) => {
                    let columns: Vec<String> = db(sqlx::query_scalar(
                        "SELECT COALESCE(name, '') FROM pragma_index_info($1) ORDER BY seqno",
                    )
                    .bind(name)
                    .fetch_all(&mut *tx))
                    .await?;
                    let definition: Option<String> = db(sqlx::query_scalar(
                            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = $1 AND tbl_name = 'jevia_runs'"
                        ).bind(name).fetch_optional(&mut *tx)).await?;
                    if columns == ["project", "ordinal"]
                        && definition.is_some_and(|sql| {
                            predicate::sqlite_matches(&sql, self.observation_predicate())
                        })
                    {
                        Present
                    } else {
                        Unavailable
                    }
                }
                Some(_) => Unavailable,
            }
        };
        db(tx.rollback()).await?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::ProjectPaths;
    use jevia_core::StorageConfig;

    #[tokio::test]
    async fn sqlite_index_status_is_read_only_and_distinguishes_missing_and_unexpected_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig::Sqlite {
            url: "sqlite://.jevia/status.db".into(),
        };
        let paths = ProjectPaths::at(dir.path().into());
        let database = Database::open(&config, &paths, true).await.unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Present
        );
        let before: Vec<(String, String)> =
            sqlx::query_as("SELECT name, COALESCE(sql, '') FROM sqlite_master ORDER BY name")
                .fetch_all(&database.pool)
                .await
                .unwrap();
        database.observation_index_status().await.unwrap();
        let after: Vec<(String, String)> =
            sqlx::query_as("SELECT name, COALESCE(sql, '') FROM sqlite_master ORDER BY name")
                .fetch_all(&database.pool)
                .await
                .unwrap();
        assert_eq!(before, after);
        sqlx::query(&format!("DROP INDEX {INDEX}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Missing
        );
        database.check().await.unwrap();
        database.check_deep().await.unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Missing
        );
        sqlx::query(&format!("CREATE INDEX {INDEX} ON jevia_runs(run_id)"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Unavailable
        );
        // Explicit init is additive, never a silent drop/replacement of this index.
        database.initialize().await.unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Unavailable
        );
        sqlx::query(&format!("DROP INDEX {INDEX}"))
            .execute(&database.pool)
            .await
            .unwrap();
        database.initialize().await.unwrap();
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Present
        );
        assert_eq!(database.check_deep().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn sqlite_wrong_predicates_are_unavailable_without_repair() {
        let dir = tempfile::tempdir().unwrap();
        let database = Database::open(
            &StorageConfig::Sqlite {
                url: "sqlite://.jevia/status.db".into(),
            },
            &ProjectPaths::at(dir.path().into()),
            true,
        )
        .await
        .unwrap();
        for predicate in [
            "0".to_owned(),
            "1".to_owned(),
            database
                .observation_predicate()
                .replace("'unknown'", "'Unknown'"),
        ] {
            let mut tx = database.pool.begin().await.unwrap();
            sqlx::query(&format!("DROP INDEX {INDEX}"))
                .execute(&mut *tx)
                .await
                .unwrap();
            let ddl =
                format!("CREATE INDEX {INDEX} ON jevia_runs(project, ordinal) WHERE {predicate}");
            sqlx::query(&ddl).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            assert_eq!(
                database.observation_index_status().await.unwrap(),
                ObservationIndexStatus::Unavailable
            );
            database.initialize().await.unwrap();
            let after: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=$1")
                .bind(INDEX)
                .fetch_one(&database.pool)
                .await
                .unwrap();
            assert_eq!(after, ddl);
            assert_eq!(database.check_deep().await.unwrap(), 0);
        }
    }

    #[tokio::test]
    #[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
    async fn postgres_index_status_is_scoped_and_preserves_the_legacy_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig::Postgres {
            url_env: "JEVIA_TEST_POSTGRES_URL".into(),
            project: format!("index-status-{}", uuid::Uuid::new_v4()),
            allow_insecure_localhost: true,
        };
        let mut database = Database::open(&config, &ProjectPaths::at(dir.path().into()), true)
            .await
            .unwrap();
        if database.supports_observation_index {
            assert_eq!(
                database.observation_index_status().await.unwrap(),
                ObservationIndexStatus::Present
            );
            // No global index changes: other PostgreSQL tests share these tables.
            assert_eq!(
                database
                    .index_status_named("jevia_nonexistent_index_for_status_test")
                    .await
                    .unwrap(),
                ObservationIndexStatus::Missing
            );
            // Unique names avoid changing the shared real index during parallel CI.
            for (predicate, expected) in [
                (
                    database.observation_predicate().to_owned(),
                    ObservationIndexStatus::Present,
                ),
                ("FALSE".to_owned(), ObservationIndexStatus::Unavailable),
                (
                    database
                        .observation_predicate()
                        .replace("'unknown'", "'Unknown'"),
                    ObservationIndexStatus::Unavailable,
                ),
            ] {
                let name = format!("jevia_predicate_test_{}", uuid::Uuid::new_v4().simple());
                sqlx::query(&format!(
                    "CREATE INDEX {name} ON jevia_runs(project, ordinal) WHERE {predicate}"
                ))
                .execute(&database.pool)
                .await
                .unwrap();
                let actual = database.index_status_named(&name).await.unwrap();
                sqlx::query(&format!("DROP INDEX {name}"))
                    .execute(&database.pool)
                    .await
                    .unwrap();
                assert_eq!(actual, expected);
            }
        }
        database.supports_observation_index = false;
        assert_eq!(
            database.observation_index_status().await.unwrap(),
            ObservationIndexStatus::Unsupported
        );
        assert_eq!(database.check_deep().await.unwrap(), 0);
    }
}
