//! Keyset-paged stats reads pin one snapshot for the boundary and every page.

use super::*;

const PAGE_SIZE: usize = 200;

impl Database {
    pub async fn visit_recent(
        &self,
        limit: usize,
        mut visit: impl FnMut(&RouteRecord) -> Result<()>,
    ) -> Result<bool> {
        let mut tx = self.read_snapshot().await?;
        let mut cursor = self.recent_boundary(&mut tx, limit).await?;
        let has_older_records = cursor.is_some();
        let mut remaining = limit;
        while remaining > 0 {
            let rows = self
                .recent_page(&mut tx, cursor, remaining.min(PAGE_SIZE))
                .await?;
            if rows.is_empty() {
                break;
            }
            for (ordinal, raw) in rows {
                visit(&decode(&raw)?)?;
                cursor = Some(ordinal);
                remaining -= 1;
            }
        }
        db(tx.rollback()).await?;
        Ok(has_older_records)
    }

    async fn recent_boundary(
        &self,
        tx: &mut Transaction<'_, Any>,
        limit: usize,
    ) -> Result<Option<i64>> {
        let row: Option<(i64, String)> = db(sqlx::query_as(
            "SELECT ordinal, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal DESC LIMIT 1 OFFSET $2",
        )
        .bind(&self.project)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_optional(&mut **tx)).await?;
        // Match the previous limit+1 read: validate the older sentinel as well.
        row.map(|(ordinal, raw)| decode(&raw).map(|_| ordinal))
            .transpose()
    }

    async fn recent_page(
        &self,
        tx: &mut Transaction<'_, Any>,
        cursor: Option<i64>,
        limit: usize,
    ) -> Result<Vec<(i64, String)>> {
        let limit = i64::try_from(limit.min(PAGE_SIZE)).unwrap();
        match cursor {
            None => db(sqlx::query_as("SELECT ordinal, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                .bind(&self.project).bind(limit).fetch_all(&mut **tx)).await,
            Some(cursor) => db(sqlx::query_as("SELECT ordinal, record FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                .bind(&self.project).bind(cursor).bind(limit).fetch_all(&mut **tx)).await,
        }
    }
}

#[cfg(test)]
mod tests;
