use std::{
    collections::HashSet,
    io::{BufRead, BufReader},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use jevia_core::{RECORD_SCHEMA_VERSION, RouteRecord};

use super::{LockMode, acquire_lock};

/// Basic health still validates every record, without retaining the whole history.
pub fn count(path: &Path) -> Result<usize> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok(0);
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let mut count = 0;
    super::read_records(path, |_| {
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// Retain IDs, not entire records. The ID set is required to detect duplicates
/// anywhere in a JSONL file; unlike SQL there is no primary-key constraint.
pub fn check_deep(path: &Path) -> Result<usize> {
    let parent = path
        .parent()
        .context("run history path has no parent directory")?;
    if !parent.exists() {
        return Ok(0);
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    let Some(file) = crate::regular_file::open_optional(path)? else {
        return Ok(0);
    };
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let position = index + 1;
        let line = line.with_context(|| format!("could not read history line {position}"))?;
        if line.trim().is_empty() {
            continue;
        }
        // Never echo an invalid enum/string value from a prompt or an
        // edited/imported record, matching routine history reads.
        let record: RouteRecord = serde_json::from_str(&line).map_err(|_| {
            anyhow!("invalid history record at line {position} (contents redacted)")
        })?;
        if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
            bail!("unsupported history record schema at line {position}; no repair attempted");
        }
        if record.decision.run_id.is_empty() {
            bail!("empty run identity at line {position}; no repair attempted");
        }
        record
            .decision
            .validate()
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!("invalid routing decision at line {position} (contents redacted)")
            })?;
        if !ids.insert(record.decision.run_id) {
            bail!(
                "duplicate run identity at line {position} (contents redacted); no repair attempted"
            );
        }
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests;
