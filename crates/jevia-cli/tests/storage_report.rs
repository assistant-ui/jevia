use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("jevia").unwrap();
    cmd.current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    cmd
}

fn report(root: &Path, deep: bool, ok: bool) -> Value {
    let mut cmd = cli(root);
    cmd.args(["storage", "check", "--json"]);
    if deep {
        cmd.arg("--deep");
    }
    let output = cmd
        .assert()
        .code(if ok { 0 } else { 1 })
        .get_output()
        .clone();
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], ok);
    assert_eq!(value["check"], if deep { "deep" } else { "basic" });
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("PRIVATE")
    );
    value
}

#[test]
fn json_reports_are_versioned_private_and_do_not_initialize_or_repair() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let missing = report(root, false, false);
    assert_eq!(missing["backend"], Value::Null);
    assert_eq!(missing["records"], Value::Null);
    assert_eq!(missing["error"]["code"], "configuration_unavailable");
    assert!(!root.join(".jevia").exists());
    cli(root).arg("init").assert().success();
    let config = root.join(".jevia/config.toml");
    let original = fs::read(&config).unwrap();
    for deep in [false, true] {
        let healthy = report(root, deep, true);
        assert_eq!(healthy["backend"], "jsonl");
        assert_eq!(healthy["records"], 0);
        assert_eq!(healthy["passive_history_index"], "not_applicable");
        assert_eq!(healthy["error"], Value::Null);
    }
    assert!(!root.join(".jevia/runs.jsonl").exists());
    let history = root.join(".jevia/runs.jsonl");
    fs::write(&history, "{PRIVATE malformed").unwrap();
    for deep in [false, true] {
        let failed = report(root, deep, false);
        assert_eq!(failed["error"]["code"], "history_check_failed");
        assert_eq!(failed["records"], Value::Null);
        assert_eq!(failed["passive_history_index"], Value::Null);
    }
    assert_eq!(fs::read(&config).unwrap(), original);
    assert_eq!(fs::read_to_string(&history).unwrap(), "{PRIVATE malformed");
    fs::write(&config, "PRIVATE invalid TOML").unwrap();
    assert_eq!(
        report(root, true, false)["error"]["code"],
        "configuration_unavailable"
    );
}

#[tokio::test]
async fn sqlite_report_separates_missing_database_from_missing_performance_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/report.db".into(),
        },
        ..Config::default()
    };
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let failed = report(root, true, false);
    assert_eq!(failed["backend"], "sqlite");
    assert_eq!(failed["error"]["code"], "storage_unavailable");
    assert!(!root.join(".jevia/report.db").exists());
    cli(root).args(["storage", "init"]).assert().success();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(root.join(".jevia/report.db")),
    )
    .await
    .unwrap();
    assert_eq!(report(root, true, true)["passive_history_index"], "present");
    sqlx::query("DROP INDEX jevia_runs_observations_v1")
        .execute(&pool)
        .await
        .unwrap();
    for deep in [false, true] {
        assert_eq!(report(root, deep, true)["passive_history_index"], "missing");
    }
    let indexes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'jevia_runs_observations_v1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(indexes, 0);
    assert!(!root.join(".jevia/runs.jsonl").exists());
    pool.close().await;
}

#[test]
fn bad_database_urls_are_redacted_in_json() {
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Postgres {
            url_env: "JEVIA_REPORT_DB".into(),
            project: "PRIVATE-project".into(),
            allow_insecure_localhost: false,
        },
        ..Config::default()
    };
    fs::write(
        dir.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    let output = cli(dir.path())
        .env("JEVIA_REPORT_DB", "bad://PRIVATE-password")
        .args(["storage", "check", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["backend"], "postgres");
    assert_eq!(value["error"]["code"], "storage_unavailable");
    assert!(!String::from_utf8(output).unwrap().contains("PRIVATE"));
    assert_eq!(value["records"], json!(null));
}
