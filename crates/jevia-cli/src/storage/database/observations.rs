//! Database-maintained, additive partial index: no record/SQL schema bump and
//! no new application-maintained flag that an older writer could leave stale.
use super::*;

const INDEX: &str = "jevia_runs_observations_v1";

// Keep this equivalent to is_execution_observation && !is_learning_evidence.
// CASE protects JSON access to corrupt text. Invalid JSON stays a candidate so
// selected corrupt records fail closed in decode(), without leaking contents.
// Reuse the EXACT predicate in DDL/query for SQLite and PostgreSQL planners:
// https://www.sqlite.org/partialindex.html
// https://www.postgresql.org/docs/16/indexes-partial.html
const SQLITE_PREDICATE: &str = "CASE WHEN json_valid(record) = 0 THEN 1 ELSE
    json_type(record, '$.execution') IS NOT NULL
    AND json_type(record, '$.execution') <> 'null'
    AND json_extract(record, '$.lifecycle.state') IN ('completed', 'launch_failed', 'interrupted', 'cancelled', 'timed_out')
    AND (json_extract(record, '$.outcome') = 'unknown'
         OR COALESCE(json_extract(record, '$.outcome_evidence.source'), '') NOT IN ('manual', 'verification'))
    END";

// PostgreSQL JSON operators reject escaped NUL even in unrelated task/model
// text. Substitute only in the index projection; the original record is never
// changed. None of the eligibility enum values/keys contain NUL. Use json, not
// jsonb, so unrelated numbers do not acquire PostgreSQL numeric range limits.
const POSTGRES_PREDICATE: &str = r"CASE WHEN NOT (record IS JSON) THEN TRUE ELSE
    json_typeof(replace(record, E'\\u0000', E'\\ufffd')::json -> 'execution') IS NOT NULL
    AND json_typeof(replace(record, E'\\u0000', E'\\ufffd')::json -> 'execution') <> 'null'
    AND (replace(record, E'\\u0000', E'\\ufffd')::json #>> '{lifecycle,state}') IN ('completed', 'launch_failed', 'interrupted', 'cancelled', 'timed_out')
    AND ((replace(record, E'\\u0000', E'\\ufffd')::json ->> 'outcome') = 'unknown'
         OR COALESCE(replace(record, E'\\u0000', E'\\ufffd')::json #>> '{outcome_evidence,source}', '') NOT IN ('manual', 'verification'))
    END";

impl Database {
    fn observation_predicate(&self) -> &'static str {
        if self.is_postgres() {
            POSTGRES_PREDICATE
        } else {
            SQLITE_PREDICATE
        }
    }

    pub(super) async fn initialize_observation_index(
        &self,
        tx: &mut Transaction<'_, Any>,
    ) -> Result<()> {
        if self.supports_observation_index {
            // This runs only for explicit initialization, never from routing or
            // diagnostics. DDL/backfill is atomic with the existing init transaction.
            let query = format!(
                "CREATE INDEX IF NOT EXISTS {INDEX} ON jevia_runs(project, ordinal) WHERE {}",
                self.observation_predicate()
            );
            db(sqlx::query(&query).execute(&mut **tx)).await?;
        }
        Ok(())
    }

    fn observation_query(&self) -> String {
        format!(
            "SELECT record FROM jevia_runs WHERE project = $1 AND ({}) ORDER BY ordinal DESC LIMIT $2",
            self.observation_predicate()
        )
    }

    pub(super) async fn indexed_recent_observations(
        &self,
        limit: usize,
    ) -> Result<Vec<RouteRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows: Vec<String> = db(sqlx::query_scalar(&self.observation_query())
            .bind(&self.project)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&self.pool))
        .await?;
        // One statement supplies a consistent snapshot. Only the selected window
        // crosses the SQL boundary; deep check remains the full-store validator.
        rows.iter()
            .rev()
            .map(|raw| {
                let record = decode(raw)?;
                if !record.is_execution_observation() || record.is_learning_evidence() {
                    bail!("observation index/policy mismatch; history was not silently omitted");
                }
                Ok(record)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
