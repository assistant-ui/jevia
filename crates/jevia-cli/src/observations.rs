//! Session-local native observations. Raw input is never written to disk or forwarded.
mod adapters;
use crate::{paths::ProjectPaths, storage::Storage};
use anyhow::{Context, Result, bail};
use jevia_core::{
    HarnessEvent, HarnessEventKind as Kind, HarnessInvocation, HarnessObservations,
    MAX_HARNESS_EVENTS, ObservationMode, ObservationSource, ObservationStatus as Status,
    valid_identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const INPUT_LIMIT: u64 = 64 * 1024;
const JOURNAL_LIMIT: u64 = 512 * 1024;
pub const JOURNAL_ENV: &str = "JEVIA_OBSERVATION_JOURNAL";
const HOOKS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "PostToolUse",
    "PostToolUseFailure",
    "TaskCompleted",
    "PostModelSwitch",
    "SubagentStart",
    "SubagentStop",
];

pub fn configured_source(mode: ObservationMode, program: &str) -> Option<ObservationSource> {
    let native = Path::new(program).file_name().and_then(|n| n.to_str());
    match (mode, native) {
        (ObservationMode::ClaudeHooks, _)
        | (ObservationMode::Auto, Some("claude" | "claude.exe")) => {
            Some(ObservationSource::ClaudeHooks)
        }
        (ObservationMode::CodexHooks, _) | (ObservationMode::Auto, Some("codex" | "codex.exe")) => {
            Some(ObservationSource::CodexHooks)
        }
        (ObservationMode::OpencodePlugin, _)
        | (ObservationMode::Auto, Some("opencode" | "opencode.exe")) => {
            Some(ObservationSource::OpencodePlugin)
        }
        _ => None,
    }
}

pub fn capture_conflicts(source: ObservationSource, args: &[String]) -> bool {
    adapters::conflicts(source, args)
}

#[derive(Clone)]
pub struct Capture {
    journal: Option<PathBuf>,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    auxiliary: Vec<PathBuf>,
    initial: HarnessObservations,
}

impl Capture {
    pub async fn prepare(
        mode: ObservationMode,
        invocation: &HarnessInvocation,
        directory: &Path,
        run_id: &str,
    ) -> Self {
        let mut capture = Self {
            journal: None,
            args: invocation.args.clone(),
            environment: vec![],
            auxiliary: vec![],
            initial: HarnessObservations {
                source: None,
                status: Status::Unsupported,
                events: vec![],
                totals: None,
            },
        };
        if mode == ObservationMode::Off {
            capture.initial.status = Status::Disabled;
            return capture;
        }
        let Some(source) = configured_source(mode, &invocation.program) else {
            return capture;
        };
        capture.initial.source = Some(source);
        capture.initial.status = Status::Unavailable;
        if adapters::conflicts(source, &invocation.args) {
            eprintln!(
                "jevia: native observations unavailable: preserving custom settings, remote session, or disabled hooks; process recording remains active"
            );
            return capture;
        }
        // Auto-detection must not add unknown hook settings to an older harness.
        // Explicit claude_hooks is the contract for compatible wrapper executables.
        if mode == ObservationMode::Auto
            && !supported_version(
                &invocation.program,
                directory.parent().unwrap_or(directory),
                source,
            )
            .await
        {
            eprintln!(
                "jevia: native observations unavailable: harness version outside tested adapter contract; process recording remains active"
            );
            return capture;
        }
        if capture.install(directory, run_id).is_err() {
            eprintln!(
                "jevia: native observation setup failed (details redacted); process recording remains active"
            );
        }
        capture
    }

    fn install(&mut self, directory: &Path, run_id: &str) -> Result<()> {
        let executable = std::env::current_exe()?;
        let run_id = uuid::Uuid::parse_str(run_id)?;
        let mut file = tempfile::Builder::new()
            .prefix(&format!("jevia-events-{run_id}-"))
            .suffix(".jsonl")
            .tempfile_in(directory)?;
        let source = self.initial.source.context("missing adapter source")?;
        let mut extra = adapters::install(source, &executable, file.path(), directory)?;
        // `exec`/`review` already execute locally and do not accept the TUI's
        // --no-daemon flag. The built-in Codex preset uses `exec`.
        let local_subcommand = source == ObservationSource::CodexHooks
            && self
                .args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "exec" | "e" | "review"));
        if local_subcommand || self.args.iter().any(|arg| arg == "--no-daemon") {
            extra.args.retain(|arg| arg != "--no-daemon");
        }
        let initial = HarnessObservations {
            status: Status::NoEvents,
            ..self.initial.clone()
        };
        file.write_all(&serde_json::to_vec(&Entry::Snapshot(initial.clone()))?)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        if let Some(plugin) = extra.plugin.take() {
            self.auxiliary.push(plugin.keep()?);
        }
        let (_, path) = file.keep()?;
        // Options before a literal -- separator still belong to the harness.
        let index = self
            .args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(self.args.len());
        self.args.splice(index..index, extra.args);
        self.environment = extra.environment;
        self.journal = Some(path);
        self.initial = initial;
        Ok(())
    }

    pub fn snapshot(&self) -> HarnessObservations {
        self.snapshot_after(&self.initial)
    }

    /// A failed read is missing information, never evidence that earlier events vanished.
    pub fn snapshot_after(&self, previous: &HarnessObservations) -> HarnessObservations {
        self.read_snapshot()
            .ok()
            .filter(|next| {
                next.source == previous.source
                    && next.event_count() >= previous.event_count()
                    && next.counts().discarded_inputs >= previous.counts().discarded_inputs
            })
            .unwrap_or_else(|| HarnessObservations {
                status: Status::Partial,
                ..previous.clone()
            })
    }

    fn read_snapshot(&self) -> Result<HarnessObservations> {
        let Some(path) = &self.journal else {
            return Ok(self.initial.clone());
        };
        let _guard = journal_guard(path)?;
        let mut file = open_journal(path)?;
        read_journal(&mut file)
    }

    pub async fn checkpoint(
        &self,
        storage: &Storage,
        run_id: &str,
        previous: &mut HarnessObservations,
    ) -> Result<()> {
        if self.journal.is_none() {
            return Ok(());
        }
        let capture = self.clone();
        let snapshot = tokio::task::spawn_blocking(move || capture.read_snapshot()).await??;
        if snapshot != *previous {
            storage
                .checkpoint_observations(run_id, snapshot.clone(), true)
                .await?;
            *previous = snapshot;
        }
        Ok(())
    }

    /// Called only after the supervisor has successfully persisted terminal state.
    /// On persistence failure/crash the bounded journal remains for inspection.
    pub fn persisted(&self, saved: &HarnessObservations) {
        if let Some(path) = &self.journal {
            let cleanup = (|| -> Result<()> {
                let _guard = journal_guard(path)?;
                let snapshot = read_journal(&mut open_journal(path)?)?;
                if &snapshot != saved {
                    bail!("journal differs from persisted observations");
                }
                fs::remove_file(path)?;
                Ok(())
            })();
            if cleanup.is_err() {
                eprintln!("jevia: observation journal retained; terminal run record was saved");
                return;
            }
        }
        for path in &self.auxiliary {
            let _ = fs::remove_file(path);
        }
    }
}

/// Bounded best-effort replay. Never infer that a child stopped because its
/// supervisor/DB connection disappeared. Active records remain active.
pub async fn replay(paths: &ProjectPaths, storage: &Storage, only: Option<&str>) {
    let Ok(entries) = fs::read_dir(&paths.directory) else {
        return;
    };
    let mut candidates = std::collections::BTreeMap::<String, Vec<PathBuf>>::new();
    for entry in entries.take(4096).flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix("jevia-events-") else {
            continue;
        };
        let Some(id) = rest.get(..36) else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err()
            || !rest
                .get(36..)
                .is_some_and(|s| s.starts_with('-') && s.ends_with(".jsonl"))
            || only.is_some_and(|expected| expected != id)
        {
            continue;
        }
        candidates.entry(id.into()).or_default().push(entry.path());
    }
    for (id, files) in candidates.into_iter().take(128) {
        // Ambiguous journals need inspection; never combine different attempts.
        if files.len() != 1 {
            continue;
        }
        let Ok(_guard) = storage.execution_guard(&id).await else {
            continue;
        };
        let path = &files[0];
        let result = async {
            let _journal_guard = journal_guard(path)?;
            let mut file = open_journal(path)?;
            let snapshot = read_journal(&mut file)?;
            let record = storage.get(&id).await?;
            if record
                .execution
                .as_ref()
                .and_then(|e| e.observations.as_ref())
                != Some(&snapshot)
            {
                storage
                    .checkpoint_observations(&id, snapshot, false)
                    .await?;
            }
            // Keep the journal for active runs: surviving children can still emit.
            if record
                .lifecycle
                .as_ref()
                .is_some_and(|l| !l.state.is_active())
            {
                drop(file);
                fs::remove_file(path)?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            eprintln!("jevia: observation replay deferred; journal retained (details redacted)");
        }
    }
}

async fn supported_version(program: &str, root: &Path, source: ObservationSource) -> bool {
    use tokio::io::AsyncReadExt;
    let probe = async {
        let mut child = tokio::process::Command::new(program)
            .current_dir(root)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let mut bytes = Vec::new();
        stdout.take(256).read_to_end(&mut bytes).await.ok()?;
        if !child.wait().await.ok()?.success() {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        Some(adapters::version_supported(source, &text))
    };
    tokio::time::timeout(Duration::from_secs(2), probe)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "event", rename_all = "snake_case")]
enum Entry {
    Event(HarnessEvent),
    Discarded,
    Truncated,
    Snapshot(HarnessObservations),
}

/// Stable striped locks survive atomic journal replacement and are never
/// unlinked. At most 256 sidecars per project, independent of session length.
fn journal_guard(path: &Path) -> Result<File> {
    let parent = path.parent().context("invalid journal directory")?;
    let name = path.file_name().context("invalid journal name")?;
    let stripe = format!("{:02x}", Sha256::digest(name.as_encoded_bytes())[0]);
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(guard) = crate::lease::try_acquire(&parent.join("event-leases"), &stripe)? {
            return Ok(guard);
        }
        if std::time::Instant::now() >= deadline {
            bail!("observation lock unavailable");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn save_journal(path: &Path, observations: HarnessObservations) -> Result<()> {
    let mut bytes = serde_json::to_vec(&Entry::Snapshot(observations))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > JOURNAL_LIMIT {
        bail!("journal limit exceeded");
    }
    let parent = path.parent().context("invalid journal directory")?;
    let mut temp = tempfile::Builder::new()
        .prefix(".jevia-event-checkpoint-")
        .tempfile_in(parent)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn open_journal(path: &Path) -> Result<File> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("invalid journal")?;
    if !name.starts_with("jevia-events-")
        || !name.ends_with(".jsonl")
        || !fs::symlink_metadata(path)?.is_file()
    {
        bail!("invalid journal");
    }
    let mut options = OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        bail!("invalid journal");
    }
    Ok(file)
}

fn read_journal(file: &mut File) -> Result<HarnessObservations> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(JOURNAL_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > JOURNAL_LIMIT {
        bail!("journal limit exceeded");
    }
    let mut observations = HarnessObservations {
        source: Some(ObservationSource::ClaudeHooks),
        status: Status::NoEvents,
        events: vec![],
        totals: None,
    };
    let lines: Vec<_> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    for line in &lines {
        match serde_json::from_slice::<Entry>(line) {
            Ok(Entry::Snapshot(snapshot)) if lines.len() == 1 => return Ok(snapshot),
            Ok(Entry::Event(event))
                if event.validate().is_ok() && observations.events.len() < MAX_HARNESS_EVENTS =>
            {
                observations.events.push(event)
            }
            Ok(Entry::Discarded | Entry::Truncated) => observations.status = Status::Partial,
            _ => bail!("invalid observation journal"),
        }
    }
    if observations.status != Status::Partial && !observations.events.is_empty() {
        observations.status = Status::Recorded;
    }
    Ok(observations)
}

/// The hook is deliberately silent and always exits zero: it cannot block a tool,
/// reject a model switch, inject context, or turn observation errors into agent errors.
#[cfg(test)]
fn receive(path: &Path, input: impl Read) {
    let _ = receive_inner(path, input);
}

pub fn receive_from(path: &Path, source: &str, input: impl Read) {
    let source = match source {
        "claude_hooks" => ObservationSource::ClaudeHooks,
        "codex_hooks" => ObservationSource::CodexHooks,
        "opencode_plugin" => ObservationSource::OpencodePlugin,
        _ => return,
    };
    let _ = receive_for_source(path, source, input);
}

#[cfg(test)]
fn receive_inner(path: &Path, input: impl Read) -> Result<()> {
    receive_for_source(path, ObservationSource::ClaudeHooks, input)
}

fn receive_for_source(path: &Path, source: ObservationSource, input: impl Read) -> Result<()> {
    let mut bytes = Vec::new();
    input.take(INPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    let event = (bytes.len() as u64 <= INPUT_LIMIT)
        .then(|| serde_json::from_slice::<Value>(&bytes).ok())
        .flatten()
        .and_then(|input| normalize(source, &input));
    let _guard = journal_guard(path)?;
    let mut current = read_journal(&mut open_journal(path)?)?;
    if current.source != Some(source) {
        bail!("observation source mismatch");
    }
    if let Some(event) = event {
        current.observe(event);
    } else {
        let mut totals = current.counts();
        totals.discarded_inputs = totals.discarded_inputs.saturating_add(1);
        current.totals = Some(totals);
        current.status = Status::Partial;
    }
    save_journal(path, current)
}

fn normalize(source: ObservationSource, input: &Value) -> Option<HarnessEvent> {
    let name = input.get("hook_event_name")?.as_str()?;
    if !adapters::events(source).contains(&name) {
        return None;
    }
    let kind = match name {
        "SessionStart" => Kind::SessionStarted,
        "SessionEnd" => Kind::SessionEnded,
        "UserPromptSubmit" => Kind::TurnStarted,
        "Stop" => Kind::TurnCompleted,
        "StopFailure" => Kind::TurnFailed,
        "PostToolUse" if source == ObservationSource::ClaudeHooks => Kind::ToolSucceeded,
        "PostToolUse" => Kind::ToolCompleted,
        "PostToolUseFailure" => Kind::ToolFailed,
        "TaskCompleted" => Kind::TaskReportedComplete,
        "PostModelSwitch" => Kind::ModelChanged,
        "SubagentStart" => Kind::SubagentStarted,
        "SubagentStop" => Kind::SubagentStopped,
        "Interrupt" => Kind::TurnInterrupted,
        "ModelObserved" => Kind::ModelObserved,
        _ => return None,
    };
    let identifier = |key| {
        input
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| valid_identifier(v))
            .map(str::to_owned)
    };
    let event = HarnessEvent {
        kind,
        recorded_at_ms: u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX),
        session_id: identifier("session_id"),
        agent_id: identifier("agent_id"),
        model: identifier(if kind == Kind::ModelChanged {
            "to_model"
        } else {
            "model"
        }),
        previous_model: if kind == Kind::ModelChanged {
            identifier("from_model")
        } else {
            None
        },
        tool_name: if matches!(
            kind,
            Kind::ToolSucceeded | Kind::ToolFailed | Kind::ToolCompleted
        ) {
            identifier("tool_name")
        } else {
            None
        },
    };
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevia_core::{
        Config, DecisionSource, ExecutionEvidence, Outcome, RouteDecision, RouteRecord, RunState,
        StorageConfig,
    };

    async fn replay_contract(config: Config) {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().into());
        fs::create_dir_all(&paths.directory).unwrap();
        let storage = Storage::open(&config, &paths, true).await.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        storage
            .append(&RouteRecord::new(
                RouteDecision {
                    run_id: id.clone(),
                    tier: "fast".into(),
                    suggested_tier: "fast".into(),
                    confidence: 0.9,
                    probabilities: Default::default(),
                    fallback_applied: false,
                    jev_model: "test".into(),
                    created_at_ms: 1,
                    source: DecisionSource::Live,
                },
                None,
            ))
            .await
            .unwrap();
        let capture = Capture::prepare(
            ObservationMode::ClaudeHooks,
            &HarnessInvocation {
                program: "wrapper".into(),
                args: vec![],
                model: "requested".into(),
                verification: None,
            },
            &paths.directory,
            &id,
        )
        .await;
        let journal = capture.journal.as_ref().unwrap();
        let guard = storage.execution_guard(&id).await.unwrap();
        storage
            .state(
                &id,
                RunState::Running,
                Outcome::Unknown,
                Some(ExecutionEvidence {
                    observations: Some(capture.snapshot()),
                    harness: "claude".into(),
                    model: "requested".into(),
                    duration_ms: 0,
                    exit_code: None,
                    verification: None,
                }),
            )
            .await
            .unwrap();
        receive(
            journal,
            br#"{"hook_event_name":"Stop","model":"observed","prompt":"PRIVATE"}"#.as_slice(),
        );
        replay(&paths, &storage, None).await;
        assert!(
            storage
                .get(&id)
                .await
                .unwrap()
                .execution
                .unwrap()
                .observations
                .unwrap()
                .events
                .is_empty(),
            "live supervisor must not be replayed"
        );
        let mut previous = storage
            .get(&id)
            .await
            .unwrap()
            .execution
            .unwrap()
            .observations
            .unwrap();
        capture
            .checkpoint(&storage, &id, &mut previous)
            .await
            .unwrap();
        assert_eq!(previous.events.len(), 1);
        // A torn/unreadable final snapshot must not erase the last checkpoint.
        fs::write(journal, b"{torn snapshot\n").unwrap();
        assert!(
            capture
                .checkpoint(&storage, &id, &mut previous)
                .await
                .is_err()
        );
        let fallback = capture.snapshot_after(&previous);
        assert_eq!(fallback.event_count(), 1);
        assert_eq!(fallback.status, Status::Partial);
        capture.persisted(&fallback);
        assert!(journal.exists(), "unreadable journals remain recoverable");
        replay(&paths, &storage, None).await;
        assert!(journal.exists());
        save_journal(journal, previous.clone()).unwrap();
        // A state write with an older in-memory copy cannot overwrite durable events.
        let mut execution = storage.get(&id).await.unwrap().execution.unwrap();
        execution.observations = Some(HarnessObservations {
            status: Status::Partial,
            ..capture.initial.clone()
        });
        let saved = storage
            .state(&id, RunState::Verifying, Outcome::Unknown, Some(execution))
            .await
            .unwrap();
        let saved = saved.execution.unwrap().observations.unwrap();
        assert_eq!(saved.event_count(), 1);
        assert_eq!(saved.status, Status::Partial);
        // Simulate loss of the supervisor, not proof that its child stopped.
        drop(guard);
        receive(
            journal,
            br#"{"hook_event_name":"PostToolUseFailure","model":"observed"}"#.as_slice(),
        );
        capture.persisted(&previous);
        assert!(
            journal.exists(),
            "events arriving after a snapshot must not be deleted"
        );
        replay(&paths, &storage, None).await;
        let record = storage.get(&id).await.unwrap();
        assert_eq!(record.lifecycle.unwrap().state, RunState::Verifying);
        assert_eq!(record.outcome, Outcome::Unknown);
        assert!(record.outcome_evidence.is_none());
        assert_eq!(
            record.execution.unwrap().observations.unwrap().events.len(),
            2
        );
        assert!(journal.exists());
        replay(&paths, &storage, None).await;
        assert_eq!(storage.recent(100, false).await.unwrap().len(), 1);
        assert!(
            storage
                .checkpoint_observations(&id, previous, false)
                .await
                .is_err()
        );
        storage.recover(&id, true).await.unwrap();
        storage
            .outcome(&id, Outcome::Success, Some("manual assessment"))
            .await
            .unwrap();
        replay(&paths, &storage, None).await;
        let record = storage.get(&id).await.unwrap();
        assert_eq!(record.outcome, Outcome::Success);
        assert_eq!(record.feedback.len(), 1);
        assert_eq!(
            record.execution.unwrap().observations.unwrap().events.len(),
            2
        );
        assert!(!journal.exists());
    }

    #[tokio::test]
    async fn checkpoints_and_replay_preserve_lifecycle_outcomes_and_idempotency() {
        replay_contract(Config::default()).await;
        replay_contract(Config {
            storage: StorageConfig::Sqlite {
                url: "sqlite://.jevia/runs.db".into(),
            },
            ..Config::default()
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires isolated PostgreSQL test database"]
    async fn postgres_checkpoints_and_replay_preserve_lifecycle_outcomes_and_idempotency() {
        replay_contract(Config {
            storage: StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: uuid::Uuid::new_v4().to_string(),
                allow_insecure_localhost: true,
            },
            ..Config::default()
        })
        .await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn auto_capture_checks_version_and_uses_session_local_exec_hooks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude");
        let id = uuid::Uuid::new_v4().to_string();
        fs::write(&program, "#!/bin/sh\nprintf '2.1.251 (Claude Code)\\n'\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(supported_version("./claude", dir.path(), ObservationSource::ClaudeHooks).await);
        let invocation = HarnessInvocation {
            program: program.to_str().unwrap().into(),
            args: vec!["--print".into(), "task".into()],
            model: "requested".into(),
            verification: None,
        };
        let capture = Capture::prepare(ObservationMode::Auto, &invocation, dir.path(), &id).await;
        assert_eq!(capture.snapshot().status, Status::NoEvents);
        let settings: Value = serde_json::from_str(capture.args.last().unwrap()).unwrap();
        let hook = &settings["hooks"]["PostModelSwitch"][0]["hooks"][0];
        assert!(Path::new(hook["command"].as_str().unwrap()).is_absolute());
        assert_eq!(hook["args"][0], "capture-event");
        assert!(hook["args"][2].as_str().unwrap().contains(&id));
        capture.persisted(&capture.snapshot());
        assert!(
            !fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("jevia-events-"))
        );
        fs::write(&program, "#!/bin/sh\nprintf '2.1.100 (Claude Code)\\n'\n").unwrap();
        let capture = Capture::prepare(ObservationMode::Auto, &invocation, dir.path(), &id).await;
        assert_eq!(capture.args, invocation.args);
        assert_eq!(capture.snapshot().status, Status::Unavailable);
    }

    #[tokio::test]
    async fn unsupported_disabled_and_custom_settings_preserve_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let invocation = HarnessInvocation {
            program: "other-agent".into(),
            args: vec!["task".into()],
            model: "requested".into(),
            verification: None,
        };
        for (mode, expected) in [
            (ObservationMode::Auto, Status::Unsupported),
            (ObservationMode::Off, Status::Disabled),
        ] {
            let capture = Capture::prepare(mode, &invocation, dir.path(), &id).await;
            assert_eq!(capture.args, invocation.args);
            assert_eq!(capture.snapshot().status, expected);
        }
        let invocation = HarnessInvocation {
            args: vec!["--settings=private-settings.json".into()],
            ..invocation
        };
        let capture =
            Capture::prepare(ObservationMode::ClaudeHooks, &invocation, dir.path(), &id).await;
        assert_eq!(capture.args, invocation.args);
        assert_eq!(capture.snapshot().status, Status::Unavailable);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn invalid_and_oversized_hook_input_never_persists_raw_content() {
        let journal = tempfile::Builder::new()
            .prefix("jevia-events-")
            .suffix(".jsonl")
            .tempfile()
            .unwrap()
            .into_temp_path();
        receive(&journal, b"PRIVATE_INVALID_JSON".as_slice());
        receive(&journal, vec![b'x'; INPUT_LIMIT as usize + 1].as_slice());
        let bytes = fs::read_to_string(&journal).unwrap();
        assert!(!bytes.contains("PRIVATE_"));
        let mut file = open_journal(&journal).unwrap();
        let snapshot = read_journal(&mut file).unwrap();
        assert_eq!(snapshot.status, Status::Partial);
        assert_eq!(snapshot.counts().discarded_inputs, 2);
    }

    #[test]
    fn concurrent_tool_hooks_are_serialized_without_lost_events() {
        let dir = tempfile::tempdir().unwrap();
        let journal = tempfile::Builder::new()
            .prefix("jevia-events-")
            .suffix(".jsonl")
            .tempfile_in(dir.path())
            .unwrap()
            .into_temp_path();
        std::thread::scope(|scope| {
            for i in 0..8 {
                let path = &journal;
                scope.spawn(move || {
                    let raw = json!({"hook_event_name": "PostToolUse", "session_id": format!("session-{i}"), "tool_name": "Read"}).to_string();
                    receive_inner(path, raw.as_bytes()).unwrap();
                });
            }
        });
        let captured = read_journal(&mut open_journal(&journal).unwrap()).unwrap();
        assert_eq!(captured.events.len(), 8);
        assert_eq!(captured.status, Status::Recorded);
    }

    #[test]
    fn capture_is_allowlisted_bounded_and_does_not_infer_success() {
        let dir = tempfile::tempdir().unwrap();
        let journal = tempfile::Builder::new()
            .prefix("jevia-events-")
            .suffix(".jsonl")
            .tempfile_in(dir.path())
            .unwrap()
            .into_temp_path();
        let raw = json!({"hook_event_name":"Stop", "session_id":"session-1", "last_assistant_message":"PRIVATE_OUTPUT", "prompt":"PRIVATE_PROMPT", "transcript_path":"/PRIVATE_PATH"}).to_string();
        receive(&journal, raw.as_bytes());
        let saved = fs::read_to_string(&journal).unwrap();
        assert!(!saved.contains("PRIVATE_"));
        let mut file = open_journal(&journal).unwrap();
        let summary = read_journal(&mut file).unwrap();
        drop(file);
        assert_eq!(summary.events[0].kind, Kind::TurnCompleted);
        assert_eq!(summary.events[0].model, None);
        assert!(!summary.routing_summary().to_string().contains("session-1"));
        for _ in 0..MAX_HARNESS_EVENTS + 3 {
            receive_inner(&journal, raw.as_bytes()).unwrap();
        }
        receive(&journal, b"malformed PRIVATE_INPUT".as_slice());
        let summary = read_journal(&mut open_journal(&journal).unwrap()).unwrap();
        assert_eq!(summary.events.len(), MAX_HARNESS_EVENTS);
        assert_eq!(summary.status, Status::Partial);
        assert_eq!(summary.event_count(), MAX_HARNESS_EVENTS as u64 + 4);
        receive(
            &journal,
            br#"{"hook_event_name":"PostModelSwitch","from_model":"early","to_model":"late"}"#
                .as_slice(),
        );
        let summary = read_journal(&mut open_journal(&journal).unwrap()).unwrap();
        assert_eq!(
            summary.events.last().unwrap().model.as_deref(),
            Some("late")
        );
        assert_eq!(
            summary.routing_summary()["event_counts"]["turn_completed"],
            MAX_HARNESS_EVENTS + 4
        );
        assert_eq!(
            summary.routing_summary()["model_event_counts"]["late"]["model_changed"],
            1
        );
        assert!(fs::metadata(&journal).unwrap().len() < JOURNAL_LIMIT);
        assert!(
            summary.routing_summary()["summary_truncated"]
                .as_bool()
                .unwrap()
        );
    }
}
