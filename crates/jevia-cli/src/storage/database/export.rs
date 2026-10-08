//! Bounded exports use a database snapshot, not independently changing pages.

use std::io::Write;

use anyhow::{Result, anyhow, bail};
use futures_util::TryStreamExt;
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
            let (last, visited) = self
                .visit_export_page(&mut tx, cursor, |record| {
                    output.write_all(&crate::jsonl::encode(record)?)?;
                    Ok(())
                })
                .await?;
            if visited == 0 {
                break;
            }
            count += visited;
            cursor = last;
        }
        // No database mutation to commit. Release the snapshot before publishing
        // the file, and fail closed if the transaction cannot finish cleanly.
        db(tx.rollback()).await?;
        Ok(count)
    }

    async fn visit_export_page(
        &self,
        tx: &mut Transaction<'_, Any>,
        cursor: Option<i64>,
        mut visit: impl FnMut(&RouteRecord) -> Result<()>,
    ) -> Result<(Option<i64>, usize)> {
        let query = match cursor {
            None => sqlx::query("SELECT run_id, ordinal, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                .bind(&self.project).bind(PAGE_SIZE),
            Some(cursor) => sqlx::query("SELECT run_id, ordinal, record FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                .bind(&self.project).bind(cursor).bind(PAGE_SIZE),
        };
        // Keep the snapshot/keyset ordering, but never retain a page of payloads.
        let mut rows = query.fetch(&mut **tx);
        let mut last = None;
        let mut count = 0;
        while let Some(row) = db(rows.try_next()).await? {
            let fields = || -> sqlx::Result<(String, i64, String)> {
                Ok((
                    row.try_get("run_id")?,
                    row.try_get("ordinal")?,
                    row.try_get("record")?,
                ))
            };
            let (id, ordinal, raw) = fields()
                .map_err(|_| anyhow!("invalid database row; export refused (contents redacted)"))?;
            let record = decode(&raw)?;
            if record.decision.run_id != id {
                bail!("stored run identity does not match its index; export refused");
            }
            visit(&record)?;
            last = Some(ordinal);
            count += 1;
        }
        Ok((last, count))
    }
}

#[cfg(test)]
mod tests;
