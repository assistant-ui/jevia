use assert_cmd::Command;
use jevia_core::{
    Config, DecisionSource, ExecutionEvidence, HarnessEvent, HarnessEventKind, HarnessObservations,
    ObservationMode, ObservationSource, ObservationStatus, RouteDecision, RouteRecord,
};
use serde_json::Value;
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("jevia").unwrap();
    cmd.current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    cmd
}

#[test]
fn recording_health_is_bounded_redacted_and_does_not_replay_or_launch() {
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    cli(dir.path())
        .args([
            "harness",
            "setup",
            "codex",
            "--preset",
            "codex",
            "--model",
            "fast=test",
            "--model",
            "balanced=test",
            "--model",
            "strong=test",
            "--apply",
        ])
        .assert()
        .success();
    let paths = dir.path().join(".jevia");
    let mut config =
        Config::from_toml(&fs::read_to_string(paths.join("config.toml")).unwrap()).unwrap();
    let harness = config.harnesses.get_mut("codex").unwrap();
    harness.observations = ObservationMode::Off;
    harness.command = "PRIVATE-never-execute-this".into();
    fs::write(paths.join("config.toml"), config.to_toml().unwrap()).unwrap();
    let mut observed = HarnessObservations {
        source: Some(ObservationSource::CodexHooks),
        status: ObservationStatus::NoEvents,
        events: vec![],
        totals: None,
    };
    for timestamp in 1..=300 {
        observed.observe(HarnessEvent {
            kind: HarnessEventKind::ToolCompleted,
            recorded_at_ms: timestamp,
            session_id: Some("PRIVATE-session".into()),
            agent_id: None,
            model: Some("PRIVATE-model".into()),
            previous_model: None,
            tool_name: Some("PRIVATE-tool".into()),
        });
    }
    let mut record = RouteRecord::new(
        RouteDecision {
            run_id: "health-run".into(),
            tier: "fast".into(),
            suggested_tier: "fast".into(),
            confidence: 1.0,
            probabilities: Default::default(),
            fallback_applied: false,
            jev_model: "test".into(),
            created_at_ms: 1,
            source: DecisionSource::Live,
        },
        Some("PRIVATE-task".into()),
    );
    record.execution = Some(ExecutionEvidence {
        harness: "codex".into(),
        model: "PRIVATE-requested".into(),
        duration_ms: 0,
        exit_code: None,
        verification: None,
        observations: Some(observed),
    });
    let history = serde_json::to_vec(&record).unwrap();
    fs::write(paths.join("runs.jsonl"), &history).unwrap();
    let journal = paths.join("jevia-events-00000000-0000-0000-0000-000000000000-test.jsonl");
    fs::write(&journal, "PRIVATE journal").unwrap();
    let output = cli(dir.path())
        .args(["harness", "health", "codex", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!String::from_utf8_lossy(&output).contains("PRIVATE"));
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["configuration"], "disabled");
    assert_eq!(report["latest"]["status"], "recorded");
    assert_eq!(report["latest"]["event_count"], 300);
    assert_eq!(report["latest"]["last_sampled_event_at_ms"], 300);
    assert_eq!(report["latest"]["sampled"], true);
    assert_eq!(fs::read(paths.join("runs.jsonl")).unwrap(), history);
    assert_eq!(fs::read_to_string(journal).unwrap(), "PRIVATE journal");
    fs::write(paths.join("runs.jsonl"), b"PRIVATE corrupt history").unwrap();
    let failed = cli(dir.path())
        .args(["harness", "health", "codex", "--json"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(failed.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("PRIVATE"));
    assert_eq!(
        serde_json::from_slice::<Value>(&failed.stdout).unwrap()["code"],
        "storage_unavailable"
    );
    fs::write(paths.join("runs.jsonl"), b"").unwrap();
    let empty = cli(dir.path())
        .args(["harness", "health", "codex", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&empty).unwrap()["code"],
        "no_matching_execution_in_window"
    );
}
