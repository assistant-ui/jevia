//! Session-local Claude hooks. Raw input is never written to disk or forwarded.
use crate::{paths::ProjectPaths, storage::Storage};
use anyhow::{Context, Result, bail};
use jevia_core::{
    HarnessEvent, HarnessEventKind as Kind, HarnessInvocation, HarnessObservations,
    MAX_HARNESS_EVENTS, ObservationMode, ObservationSource, ObservationStatus as Status,
    valid_identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const INPUT_LIMIT: u64 = 64 * 1024;
const JOURNAL_LIMIT: u64 = 512 * 1024;
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

#[derive(Clone)]
pub struct Capture {
    journal: Option<PathBuf>,
    pub args: Vec<String>,
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
            initial: HarnessObservations {
                source: None,
                status: Status::Unsupported,
                events: vec![],
            },
        };
        if mode == ObservationMode::Off {
            capture.initial.status = Status::Disabled;
            return capture;
        }
        let native = Path::new(&invocation.program)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| matches!(name, "claude" | "claude.exe"));
        if mode == ObservationMode::Auto && !native {
            return capture;
        }
        capture.initial.source = Some(ObservationSource::ClaudeHooks);
        capture.initial.status = Status::Unavailable;
        if invocation.args.iter().any(|a| {
            matches!(a.as_str(), "--settings" | "--bare" | "--safe-mode")
                || a.starts_with("--settings=")
        }) {
            eprintln!(
                "jevia: native observations unavailable: preserving custom settings or disabled hooks; process recording remains active"
            );
            return capture;
        }
        // Auto-detection must not add unknown hook settings to an older harness.
        // Explicit claude_hooks is the contract for compatible wrapper executables.
        if mode == ObservationMode::Auto
            && !supported_version(&invocation.program, directory.parent().unwrap_or(directory))
                .await
        {
            eprintln!(
                "jevia: native observations unavailable: Claude Code 2.1.251+ required; process recording remains active"
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
        let file = tempfile::Builder::new()
            .prefix(&format!("jevia-events-{run_id}-"))
            .suffix(".jsonl")
            .tempfile_in(directory)?;
        let mut hooks = serde_json::Map::new();
        for name in HOOKS {
            hooks.insert((*name).into(), json!([{"hooks": [{"type": "command", "command": executable, "args": ["capture-event", "--journal", file.path()], "timeout": 2}]}]));
        }
        let settings = serde_json::to_string(&json!({"hooks": hooks}))?;
        let (_, path) = file.keep()?;
        // Options before a literal -- separator still belong to the harness.
        let index = self
            .args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(self.args.len());
        self.args
            .splice(index..index, ["--settings".into(), settings]);
        self.journal = Some(path);
        self.initial.status = Status::NoEvents;
        Ok(())
    }

    pub fn snapshot(&self) -> HarnessObservations {
        let Some(path) = &self.journal else {
            return self.initial.clone();
        };
        let result = (|| -> Result<HarnessObservations> {
            let mut file = open_journal(path)?;
            file.lock()?;
            read_journal(&mut file)
        })();
        result.unwrap_or_else(|_| HarnessObservations {
            status: Status::Partial,
            ..self.initial.clone()
        })
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
        let snapshot = tokio::task::spawn_blocking(move || capture.snapshot()).await?;
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
    pub fn persisted(&self) {
        if let Some(path) = &self.journal
            && fs::remove_file(path).is_err()
        {
            eprintln!("jevia: observation journal retained; terminal run record was saved");
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
            let mut file = open_journal(path)?;
            file.try_lock()?;
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

async fn supported_version(program: &str, root: &Path) -> bool {
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
        let version = text.split_whitespace().next()?;
        let parts = version
            .split('.')
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .ok()?;
        Some(parts.len() == 3 && parts.as_slice() >= [2, 1, 251].as_slice())
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
    };
    for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        match serde_json::from_slice::<Entry>(line) {
            Ok(Entry::Event(event))
                if event.validate().is_ok() && observations.events.len() < MAX_HARNESS_EVENTS =>
            {
                observations.events.push(event)
            }
            _ => observations.status = Status::Partial,
        }
    }
    if observations.status != Status::Partial && !observations.events.is_empty() {
        observations.status = Status::Recorded;
    }
    Ok(observations)
}

/// The hook is deliberately silent and always exits zero: it cannot block a tool,
/// reject a model switch, inject context, or turn observation errors into agent errors.
pub fn receive(path: &Path, input: impl Read) {
    let _ = receive_inner(path, input);
}

fn receive_inner(path: &Path, input: impl Read) -> Result<()> {
    let mut bytes = Vec::new();
    input.take(INPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    let event = (bytes.len() as u64 <= INPUT_LIMIT)
        .then(|| serde_json::from_slice::<Value>(&bytes).ok())
        .flatten()
        .and_then(|input| normalize(&input));
    let mut file = open_journal(path)?;
    // Serialize concurrent tool hooks; try_lock would silently drop ordinary
    // parallel activity. The harness bounds hook execution with a two-second deadline.
    file.lock()?;
    let current = read_journal(&mut file)?;
    let entry = if current.events.len() >= MAX_HARNESS_EVENTS {
        if current.status == Status::Partial {
            return Ok(());
        }
        Entry::Truncated
    } else if let Some(event) = event {
        Entry::Event(event)
    } else {
        if current.status == Status::Partial {
            return Ok(());
        }
        Entry::Discarded
    };
    let mut encoded = serde_json::to_vec(&entry)?;
    encoded.push(b'\n');
    if file.metadata()?.len() + encoded.len() as u64 > JOURNAL_LIMIT {
        return Ok(());
    }
    file.write_all(&encoded)?;
    file.sync_data()?;
    file.unlock()?;
    Ok(())
}

fn normalize(input: &Value) -> Option<HarnessEvent> {
    let kind = match input.get("hook_event_name")?.as_str()? {
        "SessionStart" => Kind::SessionStarted,
        "SessionEnd" => Kind::SessionEnded,
        "UserPromptSubmit" => Kind::TurnStarted,
        "Stop" => Kind::TurnCompleted,
        "StopFailure" => Kind::TurnFailed,
        "PostToolUse" => Kind::ToolSucceeded,
        "PostToolUseFailure" => Kind::ToolFailed,
        "TaskCompleted" => Kind::TaskReportedComplete,
        "PostModelSwitch" => Kind::ModelChanged,
        "SubagentStart" => Kind::SubagentStarted,
        "SubagentStop" => Kind::SubagentStopped,
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
        tool_name: if matches!(kind, Kind::ToolSucceeded | Kind::ToolFailed) {
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
        // Simulate loss of the supervisor, not proof that its child stopped.
        drop(guard);
        receive(
            journal,
            br#"{"hook_event_name":"PostToolUseFailure","model":"observed"}"#.as_slice(),
        );
        replay(&paths, &storage, None).await;
        let record = storage.get(&id).await.unwrap();
        assert_eq!(record.lifecycle.unwrap().state, RunState::Running);
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
        assert!(supported_version("./claude", dir.path()).await);
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
        capture.persisted();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
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
            .unwrap();
        receive(journal.path(), b"PRIVATE_INVALID_JSON".as_slice());
        receive(
            journal.path(),
            vec![b'x'; INPUT_LIMIT as usize + 1].as_slice(),
        );
        let bytes = fs::read_to_string(journal.path()).unwrap();
        assert_eq!(bytes, "{\"type\":\"discarded\"}\n");
        let mut file = open_journal(journal.path()).unwrap();
        assert_eq!(read_journal(&mut file).unwrap().status, Status::Partial);
    }

    #[test]
    fn concurrent_tool_hooks_are_serialized_without_lost_events() {
        let journal = tempfile::Builder::new()
            .prefix("jevia-events-")
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        std::thread::scope(|scope| {
            for i in 0..24 {
                let path = journal.path();
                scope.spawn(move || {
                    let raw = json!({"hook_event_name": "PostToolUse", "session_id": format!("session-{i}"), "tool_name": "Read"}).to_string();
                    receive_inner(path, raw.as_bytes()).unwrap();
                });
            }
        });
        let captured = read_journal(&mut open_journal(journal.path()).unwrap()).unwrap();
        assert_eq!(captured.events.len(), 24);
        assert_eq!(captured.status, Status::Recorded);
    }

    #[test]
    fn capture_is_allowlisted_bounded_and_does_not_infer_success() {
        let dir = tempfile::tempdir().unwrap();
        let journal = tempfile::Builder::new()
            .prefix("jevia-events-")
            .suffix(".jsonl")
            .tempfile_in(dir.path())
            .unwrap();
        let raw = json!({"hook_event_name":"Stop", "session_id":"session-1", "last_assistant_message":"PRIVATE_OUTPUT", "prompt":"PRIVATE_PROMPT", "transcript_path":"/PRIVATE_PATH"}).to_string();
        receive(journal.path(), raw.as_bytes());
        let saved = fs::read_to_string(journal.path()).unwrap();
        assert!(!saved.contains("PRIVATE_"));
        let mut file = open_journal(journal.path()).unwrap();
        let summary = read_journal(&mut file).unwrap();
        assert_eq!(summary.events[0].kind, Kind::TurnCompleted);
        assert_eq!(summary.events[0].model, None);
        assert!(!summary.routing_summary().to_string().contains("session-1"));
        for _ in 0..MAX_HARNESS_EVENTS + 3 {
            receive_inner(journal.path(), raw.as_bytes()).unwrap();
        }
        receive(journal.path(), b"malformed PRIVATE_INPUT".as_slice());
        let summary = read_journal(&mut file).unwrap();
        assert_eq!(summary.events.len(), MAX_HARNESS_EVENTS);
        assert_eq!(summary.status, Status::Partial);
        assert!(file.metadata().unwrap().len() < JOURNAL_LIMIT);
    }
}
