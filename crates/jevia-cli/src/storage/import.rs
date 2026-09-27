//! Capture and validate JSONL before taking a database write lock. The private,
//! unnamed spool is the sole source during the transaction, never a reopened path.
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Seek, Write},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use jevia_core::{RECORD_SCHEMA_VERSION, RouteRecord};

use crate::store;

pub(super) struct Snapshot {
    file: File,
}

impl Snapshot {
    pub fn capture(source: &Path) -> Result<Self> {
        // Unnamed files are removed on close, including abnormal process exit.
        // On Unix they are unlinked/private (0600); on Windows use protected temp ACLs.
        let mut file = tempfile::tempfile().context("could not create private import snapshot")?;
        store::with_import_reader(source, |reader| {
            let mut validator = Validator::default();
            let mut writer = BufWriter::new(&mut file);
            for record in read_records(reader) {
                let record = record?;
                validator.check(&record)?;
                serde_json::to_writer(&mut writer, &record)
                    .context("could not write import snapshot")?;
                writer.write_all(b"\n")?;
            }
            writer.flush().context("could not flush import snapshot")
        })?;
        // No durable backup is published. This file is only read in this process;
        // the unchanged original remains the user's backup.
        file.rewind().context("could not rewind import snapshot")?;
        Ok(Self { file })
    }

    pub fn records(self) -> impl Iterator<Item = Result<RouteRecord>> {
        read_records(BufReader::new(self.file))
    }
}

fn read_records(reader: impl BufRead) -> impl Iterator<Item = Result<RouteRecord>> {
    reader.lines().enumerate().filter_map(|(index, line)| {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                return Some(Err(anyhow!(
                    "could not read import record on line {} (contents redacted)",
                    index + 1
                )));
            }
        };
        if line.trim().is_empty() {
            return None;
        }
        // Serde errors may quote private enum values; do not retain their source.
        Some(serde_json::from_str(&line).map_err(|_| {
            anyhow!(
                "invalid import record on line {} (contents redacted)",
                index + 1
            )
        }))
    })
}

#[derive(Default)]
struct Validator {
    ids: HashSet<String>,
}

impl Validator {
    fn check(&mut self, record: &RouteRecord) -> Result<()> {
        if !(1..=RECORD_SCHEMA_VERSION).contains(&record.schema_version) {
            bail!("unsupported import record schema; no records imported");
        }
        if record.decision.run_id.is_empty() {
            bail!("import run id cannot be empty; no records imported");
        }
        if !self.ids.insert(record.decision.run_id.clone()) {
            bail!("duplicate run id in import; no records imported");
        }
        if record
            .lifecycle
            .as_ref()
            .is_some_and(|l| l.state.is_active())
        {
            bail!("source contains active runs; stop and recover them before importing");
        }
        Ok(())
    }
}

/// Guided setup already owns a captured history; share the same validation rules.
pub fn validate_import(records: &[RouteRecord]) -> Result<()> {
    let mut validator = Validator::default();
    for record in records {
        validator.check(record)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
