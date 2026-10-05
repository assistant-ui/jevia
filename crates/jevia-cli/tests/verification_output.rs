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
fn real_verifier(
    storage: StorageConfig,
    script: &str,
    expected_code: i32,
    outcome: &str,
    state: &str,
    label: &str,
    verification_code: Option<i32>,
) {
    use jevia_core::{HarnessConfig, ObservationMode, VerificationConfig};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let mut config = Config {
        storage,
        ..Config::default()
    };
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
                args: vec!["-c".into(), script.into()],
            }),
        },
    );
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    if !config.storage.is_jsonl() {
        cli(root).args(["storage", "init"]).assert().success();
    }
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
        .code(expected_code);
    let records = cli(root)
        .args(["runs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let saved = &serde_json::from_slice::<Vec<Value>>(&records).unwrap()[0];
    assert_eq!(saved["outcome"], outcome);
    assert_eq!(saved["lifecycle"]["state"], state);
    assert_eq!(saved["execution"]["verification"]["launched"], true);
    assert_eq!(
        saved["execution"]["verification"]["exit_code"],
        json!(verification_code)
    );
    let record: jevia_core::RouteRecord = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(record.is_learning_evidence(), verification_code.is_some());
    assert_eq!(
        saved["outcome_evidence"]["source"],
        if verification_code.is_some() {
            json!("verification")
        } else if state == "completed" {
            json!("process_exit")
        } else {
            Value::Null
        }
    );
    let stats = cli(root)
        .args(["stats", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stats: Value = serde_json::from_slice(&stats).unwrap();
    assert_eq!(
        stats["totals"]["unknown"],
        usize::from(verification_code.is_none())
    );
    assert_eq!(
        stats["totals"]["learning_evidence"],
        usize::from(verification_code.is_some())
    );
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
            .contains(&format!("verification={label}"))
    );
    assert_eq!(
        cli(root)
            .args(["runs", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout,
        records
    );
}

#[cfg(unix)]
fn real_verifier_contract(storage: StorageConfig) {
    for (script, code, outcome, state, label, verifier_code) in [
        ("exit 0", 0, "success", "completed", "pass", Some(0)),
        ("exit 7", 7, "failure", "completed", "fail", Some(7)),
        ("kill -TERM $$", 1, "unknown", "completed", "unknown", None),
        ("kill -KILL $$", 1, "unknown", "completed", "unknown", None),
        ("sleep 5", 124, "unknown", "timed_out", "unknown", None),
    ] {
        let mut storage = storage.clone();
        if let StorageConfig::Postgres { project, .. } = &mut storage {
            *project = format!("verification-result-{}", uuid::Uuid::new_v4());
        }
        real_verifier(storage, script, code, outcome, state, label, verifier_code);
    }
}

#[cfg(unix)]
#[test]
fn real_verifiers_keep_inconclusive_outcomes_unknown_in_jsonl_and_sqlite() {
    real_verifier_contract(StorageConfig::Jsonl);
    real_verifier_contract(StorageConfig::Sqlite {
        url: "sqlite://.jevia/runs.db".into(),
    });
}

#[cfg(unix)]
#[test]
#[ignore = "requires JEVIA_TEST_POSTGRES_URL"]
fn postgres_real_verifiers_keep_inconclusive_outcomes_unknown() {
    real_verifier_contract(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: format!("verification-result-{}", uuid::Uuid::new_v4()),
        allow_insecure_localhost: true,
    });
}
