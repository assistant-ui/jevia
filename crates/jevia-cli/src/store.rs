use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use jevia_core::{Outcome, RouteRecord};

pub fn load(path: &Path) -> Result<Vec<RouteRecord>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not open run history at {}", path.display()));
        }
    };

    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!("could not read line {} from {}", index + 1, path.display())
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: RouteRecord = serde_json::from_str(&line).with_context(|| {
            format!(
                "invalid run record on line {} of {}",
                index + 1,
                path.display()
            )
        })?;
        if record.schema_version != 1 {
            bail!(
                "unsupported run record schema {} on line {} of {}",
                record.schema_version,
                index + 1,
                path.display()
            );
        }
        records.push(record);
    }
    Ok(records)
}

pub fn append(path: &Path, record: &RouteRecord) -> Result<()> {
    let parent = path
        .parent()
        .context("run history path does not have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let file = private_file_options(true)
        .open(path)
        .with_context(|| format!("could not open {} for writing", path.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, record).context("could not encode run record")?;
    writer
        .write_all(b"\n")
        .context("could not terminate run record")?;
    writer.flush().context("could not flush run history")?;
    Ok(())
}

pub fn update_outcome(path: &Path, run_id: &str, outcome: Outcome) -> Result<RouteRecord> {
    let mut records = load(path)?;
    let updated = records
        .iter_mut()
        .find(|record| record.decision.run_id == run_id)
        .context("run id was not found in local history")?;
    updated.outcome = outcome;
    let result = updated.clone();

    let temporary = path.with_extension("jsonl.tmp");
    let file = private_file_options(false)
        .open(&temporary)
        .with_context(|| format!("could not create {}", temporary.display()))?;
    let mut writer = BufWriter::new(file);
    for record in &records {
        serde_json::to_writer(&mut writer, record).context("could not encode run record")?;
        writer
            .write_all(b"\n")
            .context("could not terminate run record")?;
    }
    writer
        .flush()
        .context("could not flush updated run history")?;
    writer
        .get_ref()
        .sync_all()
        .context("could not sync updated run history")?;
    fs::rename(&temporary, path)
        .with_context(|| format!("could not replace {}", path.display()))?;

    Ok(result)
}

fn private_file_options(append: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jevia_core::RouteDecision;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn appends_loads_and_updates_records() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("runs.jsonl");
        let record = sample_record();

        append(&path, &record).expect("record appends");
        assert_eq!(load(&path).expect("records load"), vec![record.clone()]);

        let updated = update_outcome(&path, "run-1", Outcome::Success).expect("outcome updates");
        assert_eq!(updated.outcome, Outcome::Success);
        assert_eq!(
            load(&path).expect("records reload")[0].outcome,
            Outcome::Success
        );
    }

    fn sample_record() -> RouteRecord {
        RouteRecord {
            schema_version: 1,
            decision: RouteDecision {
                run_id: "run-1".to_owned(),
                tier: "balanced".to_owned(),
                suggested_tier: "balanced".to_owned(),
                confidence: 0.8,
                probabilities: BTreeMap::new(),
                fallback_applied: false,
                jev_model: "jev-test".to_owned(),
                created_at_ms: 1,
            },
            task: Some("test task".to_owned()),
            outcome: Outcome::Unknown,
        }
    }
}
