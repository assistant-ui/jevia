use std::{fs, path::Path};

use assert_cmd::Command;
use jevia_core::{Config, RouteRecord, StorageConfig};
use serde_json::json;

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn flow(postgres: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let state = root.join(".jevia");
    cli(root).arg("init").assert().success();
    let mut config = Config {
        storage: if postgres {
            StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("stream-import-cli-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            }
        } else {
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/import.db".into(),
            }
        },
        ..Config::default()
    };
    config.jev.base_url = "http://127.0.0.1:1".into();
    let config_text = config.to_toml().unwrap();
    fs::write(state.join("config.toml"), &config_text).unwrap();
    fs::write(state.join("cache.jsonl"), "private-cache-not-json").unwrap();
    fs::write(state.join("runs.jsonl"), "stale-jsonl-not-selected").unwrap();
    cli(root).args(["storage", "init"]).assert().success();
    let records: Vec<RouteRecord> = (0..405).map(|index| serde_json::from_value(json!({
        "schema_version": 1 + index % 3, "run_id": format!("import-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 405 - index, "task": "private-import-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()).collect();
    let source = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n\r\n");
    let source_path = state.join("source.jsonl");
    fs::write(&source_path, &source).unwrap();
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let args = ["storage", "import-jsonl", "--from", ".jevia/source.jsonl"];

    let output = cli(&nested)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8_lossy(&output)
            .contains("405 records to import, 0 identical records skipped")
    );
    let empty = cli(root)
        .args(["runs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        serde_json::from_slice::<Vec<RouteRecord>>(&empty)
            .unwrap()
            .is_empty()
    );
    let mut active = records[0].clone();
    active.decision.run_id = "active".into();
    active.lifecycle.as_mut().unwrap().state = jevia_core::RunState::Running;
    let invalid_enum = serde_json::to_string(&records[0])
        .unwrap()
        .replace("\"success\"", "\"private-invalid-enum\"");
    for tail in [
        "{private-broken-tail".to_owned(),
        invalid_enum,
        serde_json::to_string(&records[0]).unwrap(),
        serde_json::to_string(&active).unwrap(),
    ] {
        let invalid = format!("{source}\n{tail}");
        fs::write(&source_path, &invalid).unwrap();
        for apply in [false, true] {
            let mut command = cli(&nested);
            command.args(args);
            if apply {
                command.arg("--apply");
            }
            let output = command.assert().failure().get_output().clone();
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-"));
            assert!(!String::from_utf8_lossy(&output.stdout).contains("private-"));
        }
        assert_eq!(fs::read_to_string(&source_path).unwrap(), invalid);
    }
    let empty = cli(root)
        .args(["runs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        serde_json::from_slice::<Vec<RouteRecord>>(&empty)
            .unwrap()
            .is_empty()
    );

    fs::write(&source_path, &source).unwrap();
    let output = cli(&nested)
        .args(args)
        .arg("--apply")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8_lossy(&output)
            .contains("405 records imported, 0 identical records skipped")
    );
    let output = cli(&nested)
        .args(args)
        .arg("--apply")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8_lossy(&output)
            .contains("0 records imported, 405 identical records skipped")
    );
    let shown = cli(root)
        .args(["runs", "--limit", "1000", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Vec<RouteRecord>>(&shown).unwrap(),
        records
    );
    cli(root)
        .args(["storage", "check", "--deep"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(source_path).unwrap(), source);
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        config_text
    );
    assert_eq!(
        fs::read_to_string(state.join("cache.jsonl")).unwrap(),
        "private-cache-not-json"
    );
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        "stale-jsonl-not-selected"
    );
    assert!(!nested.join(".jevia").exists());
}

#[test]
fn sqlite_streaming_import_cli_validates_previews_and_preserves_sources() {
    flow(false);
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_streaming_import_cli_validates_previews_and_preserves_sources() {
    flow(true);
}
