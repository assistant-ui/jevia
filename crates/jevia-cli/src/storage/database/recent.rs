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
            let (last, count) = self
                .visit_recent_page(&mut tx, cursor, remaining.min(PAGE_SIZE), &mut visit)
                .await?;
            if count == 0 {
                break;
            }
            cursor = last;
            remaining -= count;
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

    async fn visit_recent_page(
        &self,
        tx: &mut Transaction<'_, Any>,
        cursor: Option<i64>,
        limit: usize,
        visit: &mut impl FnMut(&RouteRecord) -> Result<()>,
    ) -> Result<(Option<i64>, usize)> {
        let limit = i64::try_from(limit.min(PAGE_SIZE)).unwrap();
        let query = match cursor {
            None => sqlx::query_as::<_, (i64, String)>("SELECT ordinal, record FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                .bind(&self.project).bind(limit),
            Some(cursor) => sqlx::query_as::<_, (i64, String)>("SELECT ordinal, record FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                .bind(&self.project).bind(cursor).bind(limit),
        };
        // Keep keyset pages for bounded queries, but never retain an entire page
        // of potentially large record strings. Driver prefetch remains bounded.
        let mut rows = query.fetch(&mut **tx);
        let mut last = None;
        let mut count = 0;
        while let Some((ordinal, raw)) = db(rows.try_next()).await? {
            visit(&decode(&raw)?)?;
            last = Some(ordinal);
            count += 1;
        }
        Ok((last, count))
    }
}

#[cfg(test)]
mod tests;
