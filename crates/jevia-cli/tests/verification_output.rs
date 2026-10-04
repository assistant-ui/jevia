use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn contract(storage: StorageConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let config = Config {
        storage,
        ..Config::default()
    };
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let mut cases = vec![
        ("absent", "completed", None, "none"),
        (
            "not-launched",
            "launch_failed",
            Some((false, None)),
            "unknown",
        ),
        (
            "legacy-not-launched",
            "completed",
            Some((false, Some(0))),
            "unknown",
        ),
        ("success", "completed", Some((true, Some(0))), "pass"),
        ("failure", "completed", Some((true, Some(1))), "fail"),
    ];
    for state in ["timed_out", "cancelled", "interrupted", "completed"] {
        cases.push((state, state, Some((true, None)), "unknown"));
    }
    let records: Vec<Value> = cases.iter().map(|(id, state, verifier, _)| {
        let mut row = json!({
            "schema_version":6, "run_id":id, "tier":"fast", "suggested_tier":"fast",
            "confidence":0.9, "probabilities":{}, "fallback_applied":false, "jev_model":"fixture",
            "created_at_ms":1, "source":"live", "task":null, "outcome":"unknown",
            "lifecycle":{"state":state, "started_at_ms":1, "finished_at_ms":2},
            "execution":{"harness":"fixture", "model":"fixture", "duration_ms":1, "exit_code":0}
        });
        if let Some((launched, exit_code)) = verifier {
            row["execution"]["verification"] = json!({"command":"fixture", "launched":launched, "duration_ms":1, "exit_code":exit_code});
        }
        row
    }).collect();
    let original: String = records.iter().map(|row| format!("{row}\n")).collect();
    let history = root.join(".jevia/runs.jsonl");
    fs::write(&history, &original).unwrap();
    if !config.storage.is_jsonl() {
        cli(root).args(["storage", "init"]).assert().success();
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
    }
    let output = cli(root)
        .arg("runs")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    for (id, _, _, expected) in &cases {
        let line = text
            .lines()
            .find(|line| line.starts_with(&format!("{id}  ")))
            .unwrap();
        assert!(
            line.contains(&format!("verification={expected} ")),
            "{line}"
        );
    }
    let json = cli(root)
        .args(["runs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Vec<Value>>(&json).unwrap(),
        records
    );
    assert_eq!(fs::read_to_string(history).unwrap(), original);
}

#[test]
fn jsonl_and_sqlite_verification_labels_require_a_real_exit_status() {
    contract(StorageConfig::Jsonl);
    contract(StorageConfig::Sqlite {
        url: "sqlite://.jevia/runs.db".into(),
    });
}

#[test]
#[ignore = "requires JEVIA_TEST_POSTGRES_URL"]
fn postgres_verification_labels_require_a_real_exit_status() {
    contract(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: format!("verification-output-{}", uuid::Uuid::new_v4()),
        allow_insecure_localhost: true,
    });
}

#[cfg(unix)]
#[test]
fn real_verifier_timeout_is_unknown_in_both_saved_record_and_listing() {
    use jevia_core::{HarnessConfig, ObservationMode, VerificationConfig};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let mut config = Config::default();
    config.harnesses.insert(
        "fixture".into(),
        HarnessConfig {
            command: "/bin/sh".into(),
            args: ["-c", "exit 0", "{model}", "{task}"]
                .map(String::from)
                .into(),
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "fixture".into()))
                .collect(),
            observations: ObservationMode::Off,
            auto_verify: false,
            verification: Some(VerificationConfig {
                command: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 5".into()],
            }),
        },
    );
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let key = jevia_core::route_cache_key("fixture task", Some("fixture"), &config, &[]).unwrap();
    fs::write(root.join(".jevia/cache.jsonl"), format!("{}\n", json!({
        "schema_version":1, "key":key, "created_at_ms":1, "expires_at_ms":u64::MAX,
        "decision":{"run_id":"cached", "tier":"fast", "suggested_tier":"fast", "confidence":0.9,
            "probabilities":{}, "fallback_applied":false, "jev_model":"fixture", "created_at_ms":1}
    }))).unwrap();
    cli(root)
        .args([
            "run",
            "fixture",
            "fixture task",
            "--non-interactive",
            "--verification-timeout-seconds",
            "1",
        ])
        .timeout(std::time::Duration::from_secs(15))
        .assert()
        .code(124);
    let bytes = fs::read(root.join(".jevia/runs.jsonl")).unwrap();
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["outcome"], "unknown");
    assert_eq!(saved["lifecycle"]["state"], "timed_out");
    assert_eq!(saved["execution"]["verification"]["launched"], true);
    assert!(saved["execution"]["verification"]["exit_code"].is_null());
    let output = cli(root)
        .arg("runs")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("verification=unknown")
    );
    assert_eq!(fs::read(root.join(".jevia/runs.jsonl")).unwrap(), bytes);
}
