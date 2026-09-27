//! Bounded exports use a database snapshot, not independently changing pages.

use std::io::Write;

use anyhow::{Result, anyhow, bail};
use jevia_core::RouteRecord;
use sqlx::{Any, Row, Transaction};

use super::{Database, db, decode};

const PAGE_SIZE: i64 = 200;

impl Database {
    pub async fn export(&self, mut output: impl Write) -> Result<usize> {
        let mut tx = self.read_snapshot().await?;
        let mut cursor = None;
        let mut count = 0;
        loop {
            let rows = self.export_page(&mut tx, cursor).await?;
            if rows.is_empty() {
                break;
            }
            for (ordinal, record) in rows {
                serde_json::to_writer(&mut output, &record)?;
                output.write_all(b"\n")?;
                count += 1;
                cursor = Some(ordinal);
            }
        }
        // No database mutation to commit. Release the snapshot before publishing
        // the file, and fail closed if the transaction cannot finish cleanly.
        db(tx.rollback()).await?;
        Ok(count)
    }

    async fn export_page(
        &self,
        tx: &mut Transaction<'_, Any>,
        cursor: Option<i64>,
    ) -> Result<Vec<(i64, RouteRecord)>> {
        let rows = match cursor {
            None => db(sqlx::query("SELECT run_id, ordinal, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                .bind(&self.project).bind(PAGE_SIZE).fetch_all(&mut **tx)).await?,
            Some(cursor) => db(sqlx::query("SELECT run_id, ordinal, record FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                .bind(&self.project).bind(cursor).bind(PAGE_SIZE).fetch_all(&mut **tx)).await?,
        };
        rows.into_iter()
            .map(|row| {
                let fields = || -> sqlx::Result<(String, i64, String)> {
                    Ok((
                        row.try_get("run_id")?,
                        row.try_get("ordinal")?,
                        row.try_get("record")?,
                    ))
                };
                let (id, ordinal, raw) = fields().map_err(|_| {
                    anyhow!("invalid database row; export refused (contents redacted)")
                })?;
                let record = decode(&raw)?;
                if record.decision.run_id != id {
                    bail!("stored run identity does not match its index; export refused");
                }
                Ok((ordinal, record))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
