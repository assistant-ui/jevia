use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

#[tokio::test]
async fn diagnostics_report_a_missing_index_without_building_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/index.db".into(),
        },
        ..Config::default()
    };
    let original = config.to_toml().unwrap();
    let config_path = root.join(".jevia/config.toml");
    fs::write(&config_path, &original).unwrap();
    cli(root).args(["storage", "init"]).assert().success();
    let check = cli(root)
        .args(["storage", "check"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&check).contains("passive history index: present"));
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(root.join(".jevia/index.db")),
    )
    .await
    .unwrap();
    sqlx::query("DROP INDEX jevia_runs_observations_v1")
        .execute(&pool)
        .await
        .unwrap();
    for args in [
        vec!["storage", "check"],
        vec!["storage", "check", "--deep"],
        vec!["doctor"],
    ] {
        let output = cli(root)
            .env("TYPESAFE_API_KEY", "fixture-only")
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("passive history index: missing"));
        assert!(output.contains("jevia storage init"));
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'jevia_runs_observations_v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 0, "diagnostic must not backfill the index");
    }
    assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
    assert!(!root.join(".jevia/runs.jsonl").exists());
    cli(root).args(["storage", "init"]).assert().success();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'jevia_runs_observations_v1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let sequence: i64 =
        sqlx::query_scalar("SELECT next_seq FROM jevia_projects WHERE project = 'local'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let version: i64 = sqlx::query_scalar("SELECT version FROM jevia_schema WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((sequence, version), (0, 1));
    pool.close().await;
}

#[test]
fn jsonl_diagnostics_explain_that_the_sql_index_is_not_applicable() {
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    let output = cli(dir.path())
        .args(["storage", "check", "--deep"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8_lossy(&output).contains("passive history index: not applicable (JSONL)")
    );
    assert!(!dir.path().join(".jevia/runs.jsonl").exists());
}
