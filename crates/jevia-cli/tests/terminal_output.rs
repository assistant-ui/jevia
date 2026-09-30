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

fn assert_safe(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
    assert!(text.contains("\\u{1b}"));
}

fn contract(backend: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let config = Config {
        storage: match backend {
            "sqlite" => StorageConfig::Sqlite {
                url: "sqlite://.jevia/runs.db".into(),
            },
            "postgres" => StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("terminal-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            },
            _ => StorageConfig::Jsonl,
        },
        ..Config::default()
    };
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let id = "run\u{1b}[31m\r\nspoofed";
    let tier = "tier\u{1b}[0m\u{7}";
    let model = "model\u{1b}[2J\u{8}\u{9b}";
    let records: Vec<Value> = (0..2).map(|index| {
        let mut row = json!({
            "schema_version": 6, "run_id": format!("{id}-{index}"), "tier": tier, "suggested_tier": "fast",
            "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
            "jev_model": model, "created_at_ms": 1, "task": "private", "outcome": "unknown"
        });
        if index == 0 { row["execution"] = json!({"harness": "test", "model": model, "duration_ms": 1, "exit_code": 0}); }
        row
    }).collect();
    let original: String = records.iter().map(|row| format!("{row}\n")).collect();
    let history = root.join(".jevia/runs.jsonl");
    fs::write(&history, &original).unwrap();
    if backend != "jsonl" {
        cli(root).args(["storage", "init"]).assert().success();
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
    }
    let listing = cli(root)
        .arg("runs")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_safe(&listing);
    assert_eq!(std::str::from_utf8(&listing).unwrap().lines().count(), 4);
    let json = cli(root)
        .args(["runs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: Vec<Value> = serde_json::from_slice(&json).unwrap();
    assert_eq!(parsed[0]["tier"], tier);
    assert_eq!(parsed[0]["execution"]["model"], model);
    assert_eq!(fs::read_to_string(&history).unwrap(), original);
    let first_id = format!("{id}-0");
    let feedback = cli(root)
        .args(["feedback", &first_id, "success"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_safe(&feedback);
    assert_eq!(std::str::from_utf8(&feedback).unwrap().lines().count(), 1);
    let shown = cli(root)
        .args(["runs", "show", "--", &first_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let record: Value = serde_json::from_slice(&shown).unwrap();
    assert_eq!(record["run_id"], first_id);
    assert_eq!(record["execution"]["model"], model);
    let health = cli(root)
        .args(["harness", "health", id])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_safe(&health);
}

#[test]
fn jsonl_and_sqlite_metadata_is_terminal_safe_without_changing_json() {
    contract("jsonl");
    contract("sqlite");
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_metadata_is_terminal_safe_without_changing_json() {
    contract("postgres");
}
