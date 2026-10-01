//! Explicit, recoverable maintenance. Never replay events or infer task outcomes.
use crate::{paths::ProjectPaths, storage::Storage};
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use jevia_core::{HarnessObservations, ObservationSource, RunState};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const FILE_LIMIT: u64 = 512 * 1024;

#[derive(Debug, Subcommand)]
pub enum Action {
    /// Count local recording artifacts without reading history or changing files.
    Inspect {
        #[arg(long)]
        json: bool,
    },
    /// Preview auxiliary-file cleanup; never removes journals or loss markers.
    Cleanup {
        #[arg(long, requires = "confirm_stopped")]
        apply: bool,
        /// Confirm all local/remote harnesses and hook processes have stopped.
        #[arg(long)]
        confirm_stopped: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    operation: &'static str,
    applied: bool,
    scan_complete: bool,
    scanned_entries: usize,
    artifacts: BTreeMap<&'static str, usize>,
    eligible: usize,
    moved: usize,
    retained: BTreeMap<&'static str, usize>,
    archive: Option<PathBuf>,
    #[cfg(test)]
    #[serde(skip)]
    history_reads: usize,
}

struct Artifact {
    path: PathBuf,
    kind: &'static str,
    owner: Option<String>,
}
struct Scan {
    report: Report,
    files: Vec<Artifact>,
    journals: BTreeSet<String>,
}

fn owner(name: &str, prefix: &str) -> Option<String> {
    let rest = name.strip_prefix(prefix)?;
    let id = rest.get(..36)?;
    if !rest.get(36..)?.starts_with('-') {
        return None;
    }
    uuid::Uuid::parse_str(id).ok()?;
    Some(id.into())
}

/// New auxiliary names retain ownership after a crash. Legacy unowned names are kept.
pub(crate) fn asset_prefix(prefix: &str, journal: &Path) -> String {
    journal
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| owner(n, "jevia-events-"))
        .map_or_else(|| prefix.to_owned(), |id| format!("{prefix}{id}-"))
}

fn scan(directory: &Path) -> Result<Scan> {
    let mut result = Scan {
        report: Report {
            schema_version: 1,
            operation: "inspect",
            applied: false,
            scan_complete: true,
            scanned_entries: 0,
            artifacts: BTreeMap::new(),
            eligible: 0,
            moved: 0,
            retained: BTreeMap::new(),
            archive: None,
            #[cfg(test)]
            history_reads: 0,
        },
        files: vec![],
        journals: BTreeSet::new(),
    };
    // Explicit maintenance inventories the complete directory. Payload reads
    // remain bounded; unrelated entries no longer disable recovery/cleanup.
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        result.report.scanned_entries += 1;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let (kind, prefix) = if name.starts_with("jevia-events-") && name.ends_with(".jsonl") {
            ("journal", "jevia-events-")
        } else if name.starts_with("jevia-events-") && name.ends_with(".loss") {
            ("loss_marker", "jevia-events-")
        } else if name.starts_with("jevia-observer-") && name.ends_with(".mjs") {
            ("plugin", "jevia-observer-")
        } else if name.starts_with(".jevia-event-checkpoint-") {
            ("checkpoint", ".jevia-event-checkpoint-")
        } else {
            continue;
        };
        let owner = owner(name, prefix);
        if matches!(kind, "journal" | "loss_marker")
            && let Some(id) = &owner
        {
            result.journals.insert(id.clone());
        }
        *result.report.artifacts.entry(kind).or_default() += 1;
        result.files.push(Artifact {
            path: entry.path(),
            kind,
            owner,
        });
    }
    Ok(result)
}

fn read_regular(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > FILE_LIMIT {
        bail!("unsafe recording file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("linked recording file");
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        bail!("unsafe recording file");
    }
    let mut bytes = vec![];
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        bail!("recording file too large");
    }
    Ok(bytes)
}

fn saved_observations(
    record: jevia_core::RouteRecord,
) -> std::result::Result<HarnessObservations, &'static str> {
    let state = record.lifecycle.ok_or("unknown_lifecycle")?.state;
    if state.is_active() || state == RunState::Routed {
        return Err("active_or_pending_run");
    }
    record
        .execution
        .and_then(|e| e.observations)
        .ok_or("missing_saved_observations")
}

mod history;
use history::HistoryBatch;

fn eligible(
    file: &Artifact,
    observations: &HarnessObservations,
) -> std::result::Result<Vec<u8>, &'static str> {
    let bytes = read_regular(&file.path).map_err(|_| "unsafe_or_unreadable_file")?;
    let matches = if file.kind == "plugin" {
        observations.source == Some(ObservationSource::OpencodePlugin)
            && bytes == include_bytes!("observations/opencode.mjs")
    } else {
        crate::observations::checkpoint_matches(&bytes, observations)
    };
    if !matches {
        return Err("contents_not_known_saved");
    }
    Ok(bytes)
}

fn archive_directory(directory: &Path) -> Result<PathBuf> {
    let parent = directory.join("recording-archives");
    match fs::symlink_metadata(&parent) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&parent)?;
        }
        _ => bail!("invalid recording archive directory"),
    }
    // Keep immediately: a later failure must never delete already-moved files.
    Ok(tempfile::Builder::new()
        .prefix("cleanup-")
        .tempdir_in(parent)?
        .keep())
}

async fn cleanup(paths: &ProjectPaths, storage: &Storage, apply: bool) -> Result<Report> {
    let mut plan = scan(&paths.directory)?;
    plan.report.operation = "cleanup";
    if !plan.report.scan_complete {
        bail!("recording scan limit reached; no files moved; inspect the directory manually");
    }
    if apply {
        crate::ensure_local_ignore(&paths.directory.join(".gitignore"))?;
    }
    plan.files
        .sort_by(|a, b| a.owner.cmp(&b.owner).then(a.path.cmp(&b.path)));
    // JSONL scans once per bounded owner batch. SQL already has indexed point
    // lookups; retain one owner/connection there. Apply leases cover the entire
    // batch snapshot and all moves; no history lock is held during filesystem work.
    let mut batch = HistoryBatch::default();
    for (index, file) in plan.files.iter().enumerate() {
        // Never acquire leases or query storage for artifacts we cannot move.
        let retain = if matches!(file.kind, "journal" | "loss_marker") {
            Some("replay_source")
        } else if file.owner.is_none() {
            Some("unknown_ownership")
        } else {
            None
        };
        if let Some(reason) = retain {
            *plan.report.retained.entry(reason).or_default() += 1;
            continue;
        }
        let id = file.owner.as_ref().expect("owned auxiliary artifact");
        if !batch.saved.contains_key(id) {
            let limit = if matches!(storage, Storage::Jsonl(_)) {
                crate::store::LOOKUP_BATCH_SIZE
            } else {
                1
            };
            let mut ids = BTreeSet::new();
            for candidate in &plan.files[index..] {
                if !matches!(candidate.kind, "journal" | "loss_marker")
                    && let Some(owner) = &candidate.owner
                {
                    ids.insert(owner.clone());
                    if ids.len() == limit {
                        break;
                    }
                }
            }
            drop(std::mem::take(&mut batch));
            batch = HistoryBatch::load(storage, ids, apply).await;
            #[cfg(test)]
            {
                plan.report.history_reads += batch.reads;
            }
        }
        if matches!(batch.saved.get(id), Some(Err("execution_busy"))) {
            *plan.report.retained.entry("execution_busy").or_default() += 1;
            continue;
        }
        let refreshed;
        let journals = if apply {
            refreshed = scan(&paths.directory)?;
            if !refreshed.report.scan_complete {
                bail!("recording scan changed; any prior moves remain in the recording archive");
            }
            &refreshed.journals
        } else {
            &plan.journals
        };
        if journals.contains(id) {
            *plan
                .report
                .retained
                .entry("pending_journal_or_marker")
                .or_default() += 1;
            continue;
        }
        let result = match batch.saved.get(id).expect("loaded owner batch") {
            Ok(observations) => eligible(file, observations),
            Err(reason) => Err(*reason),
        };
        match result {
            Err(reason) => *plan.report.retained.entry(reason).or_default() += 1,
            Ok(bytes) => {
                plan.report.eligible += 1;
                if apply {
                    if read_regular(&file.path).ok().as_ref() != Some(&bytes) {
                        *plan.report.retained.entry("changed_file").or_default() += 1;
                        continue;
                    }
                    let archive = match &plan.report.archive {
                        Some(path) => path.clone(),
                        None => {
                            let path = archive_directory(&paths.directory)?;
                            plan.report.archive = Some(path.clone());
                            path
                        }
                    };
                    let destination =
                        archive.join(file.path.file_name().context("invalid artifact name")?);
                    if destination.try_exists()? {
                        bail!("recording archive destination already exists");
                    }
                    fs::rename(&file.path, &destination).context("could not move recording artifact; prior moves remain in recording-archives")?;
                    plan.report.moved += 1;
                    #[cfg(unix)]
                    {
                        fs::File::open(&archive)?.sync_all()?;
                        fs::File::open(&paths.directory)?.sync_all()?;
                    }
                }
            }
        }
    }
    plan.report.applied = apply;
    Ok(plan.report)
}

pub async fn run(paths: &ProjectPaths, action: Action) -> Result<std::process::ExitCode> {
    let (report, json) = match action {
        Action::Inspect { json } => (scan(&paths.directory)?.report, json),
        Action::Cleanup {
            apply,
            confirm_stopped,
            json,
        } => {
            if apply && !confirm_stopped {
                bail!("cleanup apply requires --confirm-stopped");
            }
            let storage = Storage::open(&crate::load_config(paths)?, paths, false).await?;
            (cleanup(paths, &storage, apply).await?, json)
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Recording {}: {:?}; scan complete: {}",
            report.operation, report.artifacts, report.scan_complete
        );
        println!(
            "Eligible auxiliary files: {}; moved: {}; retained: {:?}",
            report.eligible, report.moved, report.retained
        );
        if let Some(path) = &report.archive {
            println!(
                "Recoverable recording archive: {}",
                crate::terminal::path(path)
            );
        }
        if report.operation == "cleanup" && !report.applied {
            println!(
                "Preview only. Stop all harnesses/hooks, then use --apply --confirm-stopped. Journals and unproven files are retained."
            );
        }
    }
    Ok(if report.scan_complete {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests;
