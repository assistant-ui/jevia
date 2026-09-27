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

fn flow(backend: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let state = root.join(".jevia");
    cli(root).arg("init").assert().success();
    let mut config = Config {
        storage: match backend {
            "jsonl" => StorageConfig::Jsonl,
            "sqlite" => StorageConfig::Sqlite {
                url: "sqlite://.jevia/export.db".into(),
            },
            "postgres" => StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("export-cli-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            },
            _ => unreachable!(),
        },
        ..Config::default()
    };
    config.jev.base_url = "http://127.0.0.1:1".into();
    fs::write(state.join("config.toml"), config.to_toml().unwrap()).unwrap();
    let expected: Vec<RouteRecord> = (0..405).map(|index| serde_json::from_value(json!({
        "schema_version": 3, "run_id": format!("export-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 405 - index, "task": "private-export-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 2, "reason": "private-reason"}]
    })).unwrap()).collect();
    let source: String = expected
        .iter()
        .map(|record| format!("{}\n", serde_json::to_string(record).unwrap()))
        .collect();
    fs::write(state.join("runs.jsonl"), &source).unwrap();
    fs::write(state.join("cache.jsonl"), "private-cache-not-json").unwrap();
    if backend != "jsonl" {
        cli(root).args(["storage", "init"]).assert().success();
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
        fs::write(state.join("runs.jsonl"), "stale-jsonl-not-selected").unwrap();
    }
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let stdout = cli(&nested)
        .args(["storage", "export", "--output", ".jevia/snapshot.jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&stdout).contains("Exported 405 records"));
    assert!(!String::from_utf8_lossy(&stdout).contains("private-"));
    let output = fs::read_to_string(state.join("snapshot.jsonl")).unwrap();
    let actual: Vec<RouteRecord> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(actual, expected);
    assert!(!nested.join(".jevia").exists());
    cli(&nested)
        .args(["storage", "export", "--output", ".jevia/snapshot.jsonl"])
        .assert()
        .failure();
    cli(&nested)
        .args(["storage", "export", "--output", ".jevia"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(state.join("snapshot.jsonl")).unwrap(),
        output
    );
    assert_eq!(
        fs::read_to_string(state.join("cache.jsonl")).unwrap(),
        "private-cache-not-json"
    );
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        if backend == "jsonl" {
            &source
        } else {
            "stale-jsonl-not-selected"
        }
    );
    if backend == "jsonl" {
        // A malformed tail fails only after earlier records have been streamed.
        fs::write(state.join("runs.jsonl"), format!("{source}{{broken-tail")).unwrap();
        cli(root)
            .args(["storage", "export", "--output", ".jevia/failed.jsonl"])
            .assert()
            .failure();
        assert!(!state.join("failed.jsonl").exists());
        assert!(!fs::read_dir(&state).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".jevia-export-")
        }));
        fs::write(state.join("runs.jsonl"), &source).unwrap();
        cli(root)
            .args(["storage", "export", "--output", ".jevia/retry.jsonl"])
            .assert()
            .success();
    } else {
        cli(root)
            .args([
                "storage",
                "import-jsonl",
                "--from",
                ".jevia/snapshot.jsonl",
                "--apply",
            ])
            .assert()
            .success();
        let shown = cli(root)
            .args(["runs", "--limit", "1000", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(
            serde_json::from_slice::<Vec<RouteRecord>>(&shown).unwrap(),
            expected
        );
    }
}

#[test]
fn jsonl_streaming_export_cli_is_atomic_and_preserves_history() {
    flow("jsonl");
}

#[test]
fn sqlite_streaming_export_cli_uses_selected_history_and_round_trips() {
    flow("sqlite");
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_streaming_export_cli_uses_selected_history_and_round_trips() {
    flow("postgres");
}
