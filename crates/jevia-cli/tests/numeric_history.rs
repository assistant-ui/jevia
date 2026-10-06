use std::{fs, path::Path, time::Duration};

use assert_cmd::Command;
use jevia_core::{Config, MAX_SAFE_INTEGER, StorageConfig};
use serde_json::{Value, json};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env("TOKIO_WORKER_THREADS", "2")
        .env_remove("TYPESAFE_API_KEY")
        .timeout(Duration::from_secs(30));
    command
}

#[test]
fn unsafe_history_numbers_fail_before_output_routing_or_import_without_rewriting() {
    let root = tempfile::tempdir().unwrap();
    cli(root.path()).arg("init").assert().success();
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/numeric-record.json")).unwrap();
    let config_path = root.path().join(".jevia/config.toml");
    let history = root.path().join(".jevia/runs.jsonl");
    let default_config = Config::default().to_toml().unwrap();
    let sql_config = Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/numeric.db".into(),
        },
        ..Config::default()
    }
    .to_toml()
    .unwrap();
    fs::write(&config_path, &sql_config).unwrap();
    cli(root.path())
        .args(["storage", "init"])
        .assert()
        .success();
    for path in [
        "/created_at_ms",
        "/lifecycle/started_at_ms",
        "/lifecycle/finished_at_ms",
        "/outcome_evidence/recorded_at_ms",
        "/feedback/0/recorded_at_ms",
        "/execution/duration_ms",
        "/execution/verification/duration_ms",
        "/execution/observations/events/0/recorded_at_ms",
    ] {
        fs::write(&config_path, &default_config).unwrap();
        let mut input = fixture.clone();
        *input.pointer_mut(path).unwrap() = json!(MAX_SAFE_INTEGER);
        fs::write(&history, format!("{input}\n")).unwrap();
        cli(root.path())
            .args(["storage", "check", "--deep", "--json"])
            .assert()
            .success();
        let shown = cli(root.path())
            .args(["runs", "show", "numeric-boundary", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(
            serde_json::from_slice::<Value>(&shown)
                .unwrap()
                .pointer(path),
            Some(&json!(MAX_SAFE_INTEGER))
        );

        *input.pointer_mut(path).unwrap() = json!(MAX_SAFE_INTEGER + 2);
        let raw = format!("{input}\n");
        fs::write(&history, &raw).unwrap();
        for args in [
            vec!["storage", "check", "--deep", "--json"],
            vec!["runs", "show", "numeric-boundary", "--json"],
            vec!["stats", "--json"],
            vec!["route", "synthetic", "--json"],
        ] {
            let output = cli(root.path())
                .args(args)
                .assert()
                .failure()
                .get_output()
                .clone();
            for bytes in [&output.stdout, &output.stderr] {
                let text = String::from_utf8_lossy(bytes);
                assert!(!text.contains("PRIVATE"));
                assert!(!text.contains("9007199254740993"));
                assert!(
                    !text.contains("TYPESAFE_API_KEY"),
                    "routing reached credentials before validating history"
                );
            }
        }
        fs::write(&config_path, &sql_config).unwrap();
        for args in [
            vec!["storage", "import-jsonl"],
            vec!["storage", "import-jsonl", "--apply"],
        ] {
            let output = cli(root.path())
                .args(args)
                .assert()
                .failure()
                .get_output()
                .clone();
            assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE"));
        }
        let checked = cli(root.path())
            .args(["storage", "check", "--deep", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(
            serde_json::from_slice::<Value>(&checked).unwrap()["records"],
            0
        );
        assert_eq!(fs::read_to_string(&history).unwrap(), raw);
    }
}
