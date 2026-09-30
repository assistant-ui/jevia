use std::{fs, path::Path};

use assert_cmd::Command;
use serde_json::{Value, json};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

#[test]
fn inspection_is_read_only_and_does_not_parse_config_history_or_journals() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    cli(root).arg("init").assert().success();
    let state = root.join(".jevia");
    fs::write(state.join("config.toml"), "INVALID PRIVATE config").unwrap();
    fs::write(state.join("runs.jsonl"), "INVALID PRIVATE history").unwrap();
    fs::write(state.join("jevia-events-legacy.jsonl"), "PRIVATE journal").unwrap();
    fs::write(state.join("jevia-observer-legacy.mjs"), "PRIVATE plugin").unwrap();
    let snapshot = || {
        let mut files: Vec<_> = fs::read_dir(&state)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), fs::read(e.path()).unwrap())
            })
            .collect();
        files.sort();
        files
    };
    let before = snapshot();
    let result = cli(root)
        .args(["recordings", "inspect", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["artifacts"]["journal"], 1);
    assert_eq!(report["artifacts"]["plugin"], 1);
    assert_eq!(report["scan_complete"], true);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE"));
    assert_eq!(snapshot(), before);
}

#[test]
fn cleanup_requires_confirmation_and_keeps_recoverable_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    cli(root).arg("init").assert().success();
    let state = root.join(".jevia");
    let id = uuid::Uuid::new_v4().to_string();
    let observations = json!({ "source":"opencode_plugin", "status":"no_events", "events":[] });
    let record = json!({
        "schema_version":6, "run_id":id, "tier":"balanced", "suggested_tier":"balanced",
        "confidence":0.8, "probabilities":{}, "fallback_applied":false, "jev_model":"test", "created_at_ms":1,
        "source":"live", "outcome":"unknown", "feedback":[],
        "lifecycle":{"state":"completed", "started_at_ms":1, "finished_at_ms":2},
        "execution":{"harness":"opencode", "model":"requested", "duration_ms":1, "exit_code":0, "observations":observations}
    });
    let history = format!("{record}\n");
    fs::write(state.join("runs.jsonl"), &history).unwrap();
    let name = format!("jevia-observer-{id}-fixture.mjs");
    let contents = include_bytes!("../src/observations/opencode.mjs");
    fs::write(state.join(&name), contents).unwrap();
    cli(root)
        .args(["recordings", "cleanup", "--apply"])
        .assert()
        .failure();
    assert!(!state.join("recording-archives").exists());
    let result = cli(root)
        .args(["recordings", "cleanup", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(report["eligible"], 1);
    assert_eq!(report["moved"], 0);
    assert!(state.join(&name).exists());
    let result = cli(root)
        .args([
            "recordings",
            "cleanup",
            "--apply",
            "--confirm-stopped",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(report["moved"], 1);
    let archive = Path::new(report["archive"].as_str().unwrap());
    assert_eq!(fs::read(archive.join(&name)).unwrap(), contents);
    assert!(!state.join(&name).exists());
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        history
    );
}
