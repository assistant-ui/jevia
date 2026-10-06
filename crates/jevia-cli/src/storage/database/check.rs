//! Logical history integrity, not database-native physical integrity or repair.

use anyhow::{Context, Result, anyhow, bail};
use futures_util::TryStreamExt;
use sqlx::{Any, Row, Transaction};

use super::{Database, SCHEMA_VERSION, db, decode};

const PAGE_SIZE: i64 = 200;

impl Database {
    pub async fn check_deep(&self) -> Result<usize> {
        let mut tx = self.read_snapshot().await?;
        let count = self.inspect_snapshot(&mut tx).await?;
        db(tx.rollback()).await?;
        Ok(count)
    }

    async fn inspect_snapshot(&self, tx: &mut Transaction<'_, Any>) -> Result<usize> {
        let version: i64 = db(
            sqlx::query_scalar("SELECT version FROM jevia_schema WHERE id = 1")
                .fetch_one(&mut **tx),
        )
        .await?;
        if version != SCHEMA_VERSION {
            bail!("unsupported database schema version; deep check refused");
        }
        let next: Option<i64> = db(sqlx::query_scalar(
            "SELECT next_seq FROM jevia_projects WHERE project = $1",
        )
        .bind(&self.project)
        .fetch_optional(&mut **tx))
        .await?;
        let next = next.context("database project no longer exists; deep check refused")?;
        if next < 0 {
            bail!("invalid database append counter; no repair attempted");
        }
        let expected: i64 = db(sqlx::query_scalar(
            "SELECT COUNT(*) FROM jevia_runs WHERE project = $1",
        )
        .bind(&self.project)
        .fetch_one(&mut **tx))
        .await?;
        let expected = usize::try_from(expected).context("invalid database record count")?;
        let mut cursor = None;
        let mut count = 0;
        loop {
            let query = match cursor {
                None => sqlx::query("SELECT run_id, ordinal, learning, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                    .bind(&self.project).bind(PAGE_SIZE),
                Some(cursor) => sqlx::query("SELECT run_id, ordinal, learning, record FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                    .bind(&self.project).bind(cursor).bind(PAGE_SIZE),
            };
            let mut rows = query.fetch(&mut **tx);
            let mut visited = 0;
            while let Some(row) = db(rows.try_next()).await? {
                let position = count + 1;
                let fields = || -> sqlx::Result<(String, i64, i64, String)> {
                    Ok((
                        row.try_get("run_id")?,
                        row.try_get("ordinal")?,
                        row.try_get("learning")?,
                        row.try_get("record")?,
                    ))
                };
                let (id, ordinal, learning, raw) = fields().map_err(|_| {
                    anyhow!("invalid database fields at record {position} (contents redacted)")
                })?;
                if ordinal <= 0
                    || ordinal > next
                    || cursor.is_some_and(|previous| ordinal <= previous)
                {
                    bail!(
                        "invalid database append ordering at record {position}; no repair attempted"
                    );
                }
                let record = decode(&raw)
                    .with_context(|| format!("history validation failed at record {position}"))?;
                if record.decision.run_id != id {
                    bail!(
                        "run identity/index mismatch at record {position} (contents redacted); no repair attempted"
                    );
                }
                if learning != i64::from(record.is_learning_evidence()) {
                    bail!("learning index mismatch at record {position}; no repair attempted");
                }
                count += 1;
                visited += 1;
                cursor = Some(ordinal);
            }
            if visited == 0 {
                break;
            }
        }
        if count != expected {
            bail!("history count/order mismatch; deep check refused, no repair attempted");
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
