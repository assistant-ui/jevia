use std::{fs, path::Path};

use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::json;

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

async fn alter_index(backend: &StorageConfig, root: &Path, value: i64) -> i64 {
    // Fixture-only mutation models stale externally edited SQL metadata.
    match backend {
        StorageConfig::Sqlite { .. } => {
            let pool = sqlx::SqlitePool::connect_with(
                sqlx::sqlite::SqliteConnectOptions::new().filename(root.join(".jevia/check.db")),
            )
            .await
            .unwrap();
            sqlx::query("UPDATE jevia_runs SET learning = $1 WHERE run_id = 'private-run-0'")
                .bind(value)
                .execute(&pool)
                .await
                .unwrap();
            let result = sqlx::query_scalar(
                "SELECT learning FROM jevia_runs WHERE run_id = 'private-run-0'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            pool.close().await;
            result
        }
        StorageConfig::Postgres {
            url_env, project, ..
        } => {
            let pool = sqlx::PgPool::connect(&std::env::var(url_env).unwrap())
                .await
                .unwrap();
            sqlx::query("UPDATE jevia_runs SET learning = $1 WHERE project = $2 AND run_id = 'private-run-0'")
                .bind(value).bind(project).execute(&pool).await.unwrap();
            let result = sqlx::query_scalar(
                "SELECT learning FROM jevia_runs WHERE project = $1 AND run_id = 'private-run-0'",
            )
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
            pool.close().await;
            result
        }
        StorageConfig::Jsonl => unreachable!(),
    }
}

async fn flow(name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let state = root.join(".jevia");
    let config = Config {
        storage: match name {
            "jsonl" => StorageConfig::Jsonl,
            "sqlite" => StorageConfig::Sqlite {
                url: "sqlite://.jevia/check.db".into(),
            },
            "postgres" => StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("deep-cli-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            },
            _ => unreachable!(),
        },
        ..Config::default()
    };
    let original_config = config.to_toml().unwrap();
    fs::write(state.join("config.toml"), &original_config).unwrap();
    if name != "jsonl" {
        cli(root).args(["storage", "init"]).assert().success();
    }
    let empty = cli(root)
        .args(["storage", "check", "--deep"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&empty).contains("records=0, check=deep"));
    let original: String = (0..2).map(|index| format!("{}\n", json!({
        "schema_version": 3, "run_id": format!("private-run-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 1, "task": "private-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 2}
    }))).collect();
    fs::write(state.join("runs.jsonl"), &original).unwrap();
    if name != "jsonl" {
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
        fs::write(state.join("runs.jsonl"), "private-stale-jsonl").unwrap();
    }
    fs::write(state.join("cache.jsonl"), "private-not-json").unwrap();
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let valid = cli(&nested)
        .args(["storage", "check", "--deep"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(valid).unwrap(),
        format!("storage: ok (backend={name}, records=2, check=deep)\n")
    );
    let basic = cli(root)
        .args(["storage", "check"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(basic).unwrap(),
        format!("storage: ok (backend={name}, records=2)\n")
    );
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        if name == "jsonl" {
            &original
        } else {
            "private-stale-jsonl"
        }
    );
    if name == "jsonl" {
        fs::write(state.join("runs.jsonl"), original.repeat(2)).unwrap();
    } else {
        assert_eq!(alter_index(&config.storage, root, 2).await, 2);
        cli(root).args(["storage", "check"]).assert().success(); // Access probe does not inspect metadata.
    }
    let failed = cli(&nested)
        .args(["storage", "check", "--deep"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(failed.stdout.is_empty());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(!error.contains("private-"));
    assert!(error.contains(if name == "jsonl" {
        "duplicate run identity"
    } else {
        "learning index mismatch"
    }));
    // Failure is repeatable because diagnostics do not repair the original problem.
    cli(root)
        .args(["storage", "check", "--deep"])
        .assert()
        .failure();
    if name == "jsonl" {
        assert_eq!(
            fs::read_to_string(state.join("runs.jsonl")).unwrap(),
            original.repeat(2)
        );
        fs::write(state.join("runs.jsonl"), &original).unwrap();
    } else {
        alter_index(&config.storage, root, 1).await;
    }
    cli(root)
        .args(["storage", "check", "--deep"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        original_config
    );
    assert_eq!(
        fs::read_to_string(state.join("cache.jsonl")).unwrap(),
        "private-not-json"
    );
    assert!(!state.join("history-backups").exists());
    assert!(!state.join("history-archives").exists());
    assert!(!nested.join(".jevia").exists());
}

#[tokio::test]
async fn jsonl_deep_check_cli_reports_duplicates_without_leaking_or_repairing() {
    flow("jsonl").await;
}
#[tokio::test]
async fn sqlite_deep_check_cli_uses_selected_history_and_reports_index_drift() {
    flow("sqlite").await;
}
#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
async fn postgres_deep_check_cli_uses_selected_history_and_reports_index_drift() {
    flow("postgres").await;
}

#[test]
fn deep_check_does_not_initialize_missing_storage_or_require_jev_credentials() {
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    for storage in [
        StorageConfig::Sqlite {
            url: "sqlite://.jevia/missing.db".into(),
        },
        StorageConfig::Postgres {
            url_env: "JEVIA_MISSING_DEEP_CHECK_URL".into(),
            project: "missing".into(),
            allow_insecure_localhost: false,
        },
    ] {
        let config = Config {
            storage,
            ..Config::default()
        };
        fs::write(
            dir.path().join(".jevia/config.toml"),
            config.to_toml().unwrap(),
        )
        .unwrap();
        let output = cli(dir.path())
            .env_remove("JEVIA_MISSING_DEEP_CHECK_URL")
            .args(["storage", "check", "--deep"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(!String::from_utf8_lossy(&output.stderr).contains("TYPESAFE_API_KEY"));
        assert!(!dir.path().join(".jevia/missing.db").exists());
        assert!(!dir.path().join(".jevia/runs.jsonl").exists());
    }
}
