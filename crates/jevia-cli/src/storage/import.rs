//! Capture and validate JSONL before taking a database write lock. The private,
//! unnamed spool is the sole source during the transaction, never a reopened path.
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Seek, Write},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use jevia_core::{RECORD_SCHEMA_VERSION, RouteRecord};
use sha2::{Digest, Sha256};

use crate::store;

pub(crate) struct Snapshot {
    file: File,
    pub count: usize,
    pub fingerprint: [u8; 32],
}

impl Snapshot {
    pub fn capture(source: &Path) -> Result<Self> {
        store::with_import_reader(source, Self::capture_reader)
    }

    /// Setup preview must not create a history lock or any other project file.
    /// Apply requires stopped writers and checks the raw source again around SQL.
    pub fn capture_optional(source: &Path) -> Result<Option<Self>> {
        open_optional(source)?.map(Self::capture_reader).transpose()
    }

    fn capture_reader(reader: impl Read) -> Result<Self> {
        // Unnamed files are removed on close, including abnormal process exit.
        // On Unix they are unlinked/private (0600); on Windows use protected temp ACLs.
        let mut file = tempfile::tempfile().context("could not create private import snapshot")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Linux O_TMPFILE may inherit a broader mode than the named-file
            // fallback. Restrict the handle before writing any private records.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .context("could not restrict import snapshot permissions")?;
        }
        let mut reader = HashingReader {
            reader,
            hash: Sha256::new(),
        };
        let mut count = 0;
        {
            let mut validator = Validator::default();
            let mut writer = BufWriter::new(&mut file);
            for record in read_records(BufReader::new(&mut reader)) {
                let record = record?;
                validator.check(&record)?;
                writer.write_all(&crate::jsonl::encode(&record)?)?;
                count += 1;
            }
            writer.flush().context("could not flush import snapshot")?;
        }
        // No durable backup is published. This file is only read in this process;
        // the unchanged original remains the user's backup.
        file.rewind().context("could not rewind import snapshot")?;
        Ok(Self {
            file,
            count,
            fingerprint: reader.hash.finalize().into(),
        })
    }

    pub fn records(self) -> impl Iterator<Item = Result<RouteRecord>> {
        read_records(BufReader::new(self.file))
    }
}

struct HashingReader<R> {
    reader: R,
    hash: Sha256,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.reader.read(bytes)?;
        self.hash.update(&bytes[..count]);
        Ok(count)
    }
}

fn open_optional(path: &Path) -> Result<Option<File>> {
    match File::open(path) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                bail!("source JSONL history must be a regular file");
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("could not read source JSONL history"),
    }
}

pub(crate) fn source_fingerprint(path: &Path) -> Result<Option<[u8; 32]>> {
    open_optional(path)?
        .map(|file| {
            let mut reader = HashingReader {
                reader: file,
                hash: Sha256::new(),
            };
            std::io::copy(&mut reader, &mut std::io::sink())
                .context("could not read source JSONL history")?;
            Ok(reader.hash.finalize().into())
        })
        .transpose()
}

fn read_records(reader: impl BufRead) -> impl Iterator<Item = Result<RouteRecord>> {
    crate::jsonl::lines(reader)
        .enumerate()
        .filter_map(|(index, line)| {
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
        record
            .decision
            .validate()
            .map_err(|_| anyhow!("invalid import routing decision (contents redacted)"))?;
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
#[cfg(test)]
fn validate_import(records: &[RouteRecord]) -> Result<()> {
    let mut validator = Validator::default();
    for record in records {
        validator.check(record)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
