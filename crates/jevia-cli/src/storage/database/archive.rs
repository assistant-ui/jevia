//! Backup-first SQL retention. Application writes share the project lock. Bounded
//! pages and a private temporary deletion journal avoid loading all history into
//! memory or deleting anything before both recovery files are durable.

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Seek, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Any, Row, Transaction};
use tempfile::NamedTempFile;

use super::{Database, db, decode};
use crate::store::{MaintenanceReport, archivable};

// Three binds per candidate + the project stay below even SQLite's older limit.
const PAGE_SIZE: usize = 200;

#[derive(Serialize, Deserialize)]
struct ArchiveRow {
    id: String,
    ordinal: i64,
    raw: String,
    owner: String,
}

impl ArchiveRow {
    fn eligible(&self) -> Result<bool> {
        let record = decode(&self.raw)?;
        if record.decision.run_id != self.id {
            bail!("stored run identity does not match its index; archival refused");
        }
        Ok(self.owner.is_empty() && archivable(&record))
    }
}

#[derive(Default)]
struct Scan {
    total: usize,
    eligible: usize,
    hash: Sha256,
}

impl Scan {
    fn add(&mut self, row: &ArchiveRow) -> Result<bool> {
        let eligible = row.eligible()?;
        self.total += 1;
        self.eligible += usize::from(eligible);
        let encoded = serde_json::to_vec(row)?;
        self.hash.update((encoded.len() as u64).to_be_bytes());
        self.hash.update(encoded);
        Ok(eligible)
    }
}

impl Database {
    pub async fn archive(
        &self,
        directory: &Path,
        keep: usize,
        apply: bool,
    ) -> Result<MaintenanceReport> {
        if keep == 0 {
            bail!("archive --keep must be greater than zero");
        }
        // Even preview takes the same project lock and rolls it back: keyset
        // pagination must not combine multiple concurrently changing snapshots.
        let mut tx = self.write().await?;
        let mut before = Scan::default();
        let mut cursor = None;
        loop {
            let rows = self.archive_page(&mut tx, cursor).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                cursor = Some(row.ordinal);
                before.add(&row)?;
            }
        }
        let count = before.eligible.saturating_sub(keep);
        let mut report = MaintenanceReport {
            operation: "archive",
            applied: false,
            would_change: count != 0,
            retained_records: before.total - count,
            archived_records: count,
            truncated_tail_bytes: 0,
            added_final_newline: false,
            backup: None,
            archive: None,
        };
        if !apply || count == 0 {
            db(tx.rollback()).await?;
            return Ok(report);
        }

        let mut backup = Snapshot::new(&directory.join("history-backups"))?;
        let mut archive = Snapshot::new(&directory.join("history-archives"))?;
        let mut journal = tempfile::Builder::new()
            .prefix(".archive-")
            .suffix(".tmp")
            // Keep even a crash-left journal inside an ignored, private directory.
            .tempfile_in(&archive.directory)?;
        let mut remaining = count;
        let mut after = Scan::default();
        cursor = None;
        loop {
            let rows = self.archive_page(&mut tx, cursor).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                cursor = Some(row.ordinal);
                let eligible = after.add(&row)?;
                backup.record(&row.raw)?;
                if eligible && remaining != 0 {
                    archive.record(&row.raw)?;
                    serde_json::to_writer(&mut journal, &row)?;
                    journal.write_all(b"\n")?;
                    remaining -= 1;
                }
            }
        }
        // Also detect writes by external SQL clients that don't use our lock.
        if remaining != 0
            || before.total != after.total
            || before.eligible != after.eligible
            || before.hash.finalize() != after.hash.finalize()
        {
            bail!(
                "history changed while saving archive; no deletion attempted. Coordinate external database writers before retrying"
            );
        }
        let backup_path = backup.finish()?;
        let archive_path = archive.finish().with_context(|| {
            format!(
                "archive could not be finalized; no deletion attempted. Backup: {}",
                backup_path.display()
            )
        })?;
        journal.as_file_mut().rewind().with_context(|| {
            format!(
                "could not read archive journal; no deletion attempted. Backup: {}; archive: {}",
                backup_path.display(),
                archive_path.display()
            )
        })?;
        let result = self
            .delete_journal(&mut tx, journal.as_file_mut(), count)
            .await;
        if let Err(error) = result {
            let _ = db(tx.rollback()).await;
            return Err(error).with_context(|| format!("archive deletion failed; no commit requested. Inspect history before retrying. Backup: {}; archive: {}", backup_path.display(), archive_path.display()));
        }
        db(tx.commit()).await.with_context(|| format!("archive commit was not confirmed; it may have succeeded. Inspect history before restoring/retrying. Backup: {}; archive: {}", backup_path.display(), archive_path.display()))?;
        report.applied = true;
        report.backup = Some(backup_path);
        report.archive = Some(archive_path);
        Ok(report)
    }

    async fn archive_page(
        &self,
        tx: &mut Transaction<'_, Any>,
        cursor: Option<i64>,
    ) -> Result<Vec<ArchiveRow>> {
        let rows = match cursor {
            None => db(sqlx::query("SELECT run_id, ordinal, record, owner FROM jevia_runs WHERE project = $1 ORDER BY ordinal ASC LIMIT $2")
                .bind(&self.project).bind(PAGE_SIZE as i64).fetch_all(&mut **tx)).await?,
            Some(cursor) => db(sqlx::query("SELECT run_id, ordinal, record, owner FROM jevia_runs WHERE project = $1 AND ordinal > $2 ORDER BY ordinal ASC LIMIT $3")
                .bind(&self.project).bind(cursor).bind(PAGE_SIZE as i64).fetch_all(&mut **tx)).await?,
        };
        rows.into_iter()
            .map(|row| {
                let decode_row = || -> sqlx::Result<ArchiveRow> {
                    Ok(ArchiveRow {
                        id: row.try_get("run_id")?,
                        ordinal: row.try_get("ordinal")?,
                        raw: row.try_get("record")?,
                        owner: row.try_get("owner")?,
                    })
                };
                decode_row().map_err(|_| {
                    anyhow!("invalid database row; archival refused (contents redacted)")
                })
            })
            .collect()
    }

    async fn delete_journal(
        &self,
        tx: &mut Transaction<'_, Any>,
        journal: &mut File,
        expected: usize,
    ) -> Result<()> {
        let mut batch = Vec::with_capacity(PAGE_SIZE);
        let mut deleted = 0;
        for line in BufReader::new(journal).lines() {
            let row: ArchiveRow = serde_json::from_str(&line?)
                .map_err(|_| anyhow!("invalid temporary archive journal"))?;
            batch.push(row);
            if batch.len() == PAGE_SIZE {
                self.delete_batch(tx, &batch).await?;
                deleted += batch.len();
                batch.clear();
            }
        }
        if !batch.is_empty() {
            self.delete_batch(tx, &batch).await?;
            deleted += batch.len();
        }
        if deleted != expected {
            bail!("archive journal count mismatch; refusing to commit deletions");
        }
        Ok(())
    }

    async fn delete_batch(&self, tx: &mut Transaction<'_, Any>, rows: &[ArchiveRow]) -> Result<()> {
        use std::fmt::Write as _;
        if rows.is_empty() || rows.len() > PAGE_SIZE {
            bail!("invalid archive deletion batch size");
        }
        // Any's QueryBuilder emits '?' placeholders, which PostgreSQL doesn't
        // support. Numbered binds work on both backends; interpolate only indices.
        let mut sql =
            String::from("DELETE FROM jevia_runs WHERE project = $1 AND owner = '' AND (");
        for (index, row) in rows.iter().enumerate() {
            if !row.eligible()? {
                bail!("ineligible run in archive journal; refusing deletion");
            }
            if index != 0 {
                sql.push_str(" OR ");
            }
            let first = 2 + index * 3;
            write!(
                sql,
                "(run_id = ${first} AND ordinal = ${} AND record = ${})",
                first + 1,
                first + 2
            )
            .expect("writing to String cannot fail");
        }
        sql.push(')');
        let mut query = sqlx::query(&sql).bind(&self.project);
        for row in rows {
            query = query.bind(&row.id).bind(row.ordinal).bind(&row.raw);
        }
        let result = db(query.persistent(false).execute(&mut **tx)).await?;
        if result.rows_affected() != rows.len() as u64 {
            bail!(
                "archived rows changed or could not be deleted; refusing to commit partial archival"
            );
        }
        Ok(())
    }
}

struct Snapshot {
    temporary: NamedTempFile,
    directory: PathBuf,
}

impl Snapshot {
    fn new(directory: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(directory)
            .context("could not create archive snapshot directory; no deletion attempted")?;
        let temporary = tempfile::Builder::new()
            .prefix(".sql-runs-")
            .suffix(".tmp")
            .tempfile_in(directory)?;
        Ok(Self {
            temporary,
            directory: directory.to_owned(),
        })
    }

    fn record(&mut self, raw: &str) -> Result<()> {
        // Valid JSON can't contain physical CR/LF inside strings. Removing only
        // that whitespace yields JSONL while preserving unknown/additive fields,
        // number representations and escaped string content from the stored JSON.
        for part in raw.split(['\n', '\r']) {
            self.temporary.write_all(part.as_bytes())?;
        }
        self.temporary.write_all(b"\n")?;
        Ok(())
    }

    fn finish(self) -> Result<PathBuf> {
        let path = self
            .directory
            .join(format!("sql-runs-{}.jsonl", uuid::Uuid::new_v4()));
        self.temporary.as_file().sync_all()?;
        self.temporary
            .persist_noclobber(&path)
            .map_err(|e| e.error)?;
        sync_directory(&self.directory)
            .and_then(|_| {
                sync_directory(
                    self.directory
                        .parent()
                        .context("archive directory has no parent")?,
                )
            })
            .with_context(|| {
                format!(
                    "could not sync snapshot directory; no deletion attempted. Snapshot: {}",
                    path.display()
                )
            })?;
        Ok(path)
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests;
