use std::{fs, path::Path, time::Duration};

use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::{Value, json};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command.current_dir(root).env("TOKIO_WORKER_THREADS", "2");
    command
        .env_remove("TYPESAFE_API_KEY")
        .timeout(Duration::from_secs(10));
    command
}

#[test]
fn contradictory_attribution_is_rejected_before_stats_routing_or_sql_import() {
    let root = tempfile::tempdir().unwrap();
    cli(root.path()).arg("init").assert().success();
    let record = json!({
        "schema_version":6, "run_id":"fixture", "tier":"fast", "suggested_tier":"fast",
        "confidence":0.9, "probabilities":{}, "fallback_applied":false, "jev_model":"fixture",
        "created_at_ms":1, "source":"live", "task":"PRIVATE task", "outcome":"unknown",
        "lifecycle":{"state":"completed", "started_at_ms":1, "finished_at_ms":2},
        "execution":{"harness":"app", "model":"requested", "duration_ms":1, "exit_code":0,
            "observations":{"source":"application", "status":"recorded",
                "events":[{"kind":"tool_failed", "recorded_at_ms":1, "model":"PRIVATE-model-a"}],
                "totals":{"event_counts":{"tool_failed":1}, "models":{"PRIVATE-model-b":{"tool_failed":1}},
                    "unattributed_event_counts":{}, "omitted_model_event_counts":{},
                    "models_truncated":false, "discarded_inputs":0}}}
    });
    let raw = format!("{record}\n");
    let history = root.path().join(".jevia/runs.jsonl");
    fs::write(&history, &raw).unwrap();
    for args in [
        vec!["storage", "check", "--deep", "--json"],
        vec!["stats", "--json"],
        vec!["route", "fixture", "--json"],
    ] {
        let output = cli(root.path())
            .args(args)
            .assert()
            .failure()
            .get_output()
            .clone();
        for bytes in [&output.stdout, &output.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains("PRIVATE"));
        }
        assert!(!String::from_utf8_lossy(&output.stderr).contains("TYPESAFE_API_KEY"));
    }
    let config = Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/test.db".into(),
        },
        ..Config::default()
    };
    fs::write(
        root.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    cli(root.path())
        .args(["storage", "init"])
        .assert()
        .success();
    let output = cli(root.path())
        .args(["storage", "import-jsonl", "--apply"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE"));
    let stats = cli(root.path())
        .args(["stats", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&stats).unwrap()["totals"]["records"],
        0
    );
    assert_eq!(fs::read_to_string(history).unwrap(), raw);
}
