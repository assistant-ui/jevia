use assert_cmd::Command;
use jevia_core::{Config, HarnessConfig, StorageConfig};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn init(root: &Path) -> String {
    cli(root).arg("init").assert().success();
    let mut config = Config::default();
    config.router.confidence_floor = 0.73;
    config.cache.ttl_seconds = 222;
    config.privacy.store_task_text = false;
    config.harnesses.insert(
        "test".into(),
        HarnessConfig {
            command: "test-agent".into(),
            args: vec!["--model".into(), "{model}".into(), "{task}".into()],
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "test/model".into()))
                .collect(),
            verification: None,
        },
    );
    let original = format!(
        "# my custom policy\n{}\n# keep this trailing note\n",
        config.to_toml().unwrap()
    )
    .replace(
        "confidence_floor = 0.73",
        "confidence_floor = 0.73 # important threshold",
    );
    fs::write(root.join(".jevia/config.toml"), &original).unwrap();
    original
}

fn record(id: &str) -> Value {
    json!({"schema_version": 3, "run_id": id, "tier": "fast", "suggested_tier": "fast", "confidence": 0.9,
        "probabilities": {}, "fallback_applied": false, "jev_model": "test", "created_at_ms": 1,
        "task": "private-task", "outcome": "success", "outcome_evidence": {"source": "manual", "recorded_at_ms": 2}})
}

fn json_output(root: &Path, args: &[&str]) -> Value {
    let bytes = cli(root)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&bytes).unwrap()
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join(".jevia"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn flow(postgres: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    let source = format!("{}\n{}\n", record("a"), record("b"));
    fs::write(root.join(".jevia/runs.jsonl"), &source).unwrap();
    let project = format!("setup-{}", uuid::Uuid::new_v4());
    let backend = if postgres {
        vec![
            "postgres",
            "--url-env",
            "JEVIA_TEST_POSTGRES_URL",
            "--project",
            project.as_str(),
            "--allow-insecure-localhost",
        ]
    } else {
        vec!["sqlite"]
    };
    let before = files(root);
    cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--import-jsonl"])
        .assert()
        .success();
    assert_eq!(files(root), before); // Preview doesn't even create lock/backup files.
    cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--apply", "--confirm-stopped"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
        original
    );
    assert!(!root.join(".jevia/jevia.db").exists());
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let output = cli(&nested)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--import-jsonl", "--apply", "--confirm-stopped"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!String::from_utf8_lossy(&output).contains("private-task"));
    assert_eq!(
        fs::read_to_string(root.join(".jevia/runs.jsonl")).unwrap(),
        source
    );
    let updated = fs::read_to_string(root.join(".jevia/config.toml")).unwrap();
    assert!(updated.contains("# my custom policy"));
    assert!(updated.contains("# important threshold"));
    assert!(updated.contains("# keep this trailing note"));
    let mut expected = Config::from_toml(&original).unwrap();
    expected.storage = if postgres {
        StorageConfig::Postgres {
            url_env: "JEVIA_TEST_POSTGRES_URL".into(),
            project: project.clone(),
            allow_insecure_localhost: true,
        }
    } else {
        StorageConfig::Sqlite {
            url: "sqlite://.jevia/jevia.db".into(),
        }
    };
    assert_eq!(Config::from_toml(&updated).unwrap(), expected);
    let backups: Vec<_> = fs::read_dir(root.join(".jevia/config-backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
    let ignore = fs::read_to_string(root.join(".jevia/.gitignore")).unwrap();
    assert!(ignore.contains("config-backups/"));
    assert!(ignore.contains("config.lock"));
    assert_eq!(
        json_output(root, &["stats", "--json"])["totals"]["records"],
        2
    );
    let runs = json_output(root, &["runs", "--json"]);
    assert_eq!(runs.as_array().unwrap().len(), 2);
    assert_eq!(runs[0]["run_id"], "a");
    assert_eq!(runs[1]["run_id"], "b");
    // Compare full exported records rather than relying only on aggregate counts.
    cli(root)
        .args(["storage", "export", "--output", ".jevia/export.jsonl"])
        .assert()
        .success();
    let exported: Vec<Value> = fs::read_to_string(root.join(".jevia/export.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(exported.len(), 2);
    for (actual, id) in exported.iter().zip(["a", "b"]) {
        let normalized: jevia_core::RouteRecord = serde_json::from_value(record(id)).unwrap();
        assert_eq!(*actual, serde_json::to_value(normalized).unwrap());
    }
    cli(root).args(["storage", "check"]).assert().success();
    // Re-run without importing the retained (potentially stale) JSONL again.
    let backend = if postgres {
        vec![
            "postgres",
            "--url-env",
            "JEVIA_TEST_POSTGRES_URL",
            "--project",
            match &expected.storage {
                StorageConfig::Postgres { project, .. } => project,
                _ => unreachable!(),
            },
            "--allow-insecure-localhost",
        ]
    } else {
        vec!["sqlite"]
    };
    cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--apply", "--confirm-stopped"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
        updated
    );
    assert_eq!(
        fs::read_dir(root.join(".jevia/config-backups"))
            .unwrap()
            .count(),
        1
    );
    cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--import-jsonl", "--apply", "--confirm-stopped"])
        .assert()
        .failure();
    // Retry after an interrupted switch: existing identical IDs are not duplicated.
    fs::write(root.join(".jevia/config.toml"), &original).unwrap();
    let retried = cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--import-jsonl", "--apply", "--confirm-stopped"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&retried).contains("0 imported, 2 identical records skipped"));
    // A conflicting pre-existing destination must roll back all import rows and
    // leave the original JSONL configuration selected, even after a partial insert.
    let mut conflicting = record("a");
    conflicting["outcome"] = json!("failure");
    let conflicted_source = format!("{}\n{conflicting}\n", record("new-before-conflict"));
    fs::write(root.join(".jevia/config.toml"), &original).unwrap();
    fs::write(root.join(".jevia/runs.jsonl"), &conflicted_source).unwrap();
    cli(root)
        .args(["storage", "setup"])
        .args(&backend)
        .args(["--import-jsonl", "--apply", "--confirm-stopped"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(root.join(".jevia/runs.jsonl")).unwrap(),
        conflicted_source
    );
    fs::write(root.join(".jevia/config.toml"), &updated).unwrap();
    assert_eq!(
        json_output(root, &["stats", "--json"])["totals"]["records"],
        2
    );
    cli(root)
        .args([
            "storage",
            "setup",
            "sqlite",
            "--path",
            ".jevia/other.db",
            "--apply",
            "--confirm-stopped",
        ])
        .assert()
        .failure();
    assert!(!root.join(".jevia/other.db").exists());
}

#[test]
fn sqlite_setup_previews_imports_preserves_policy_and_verifies() {
    flow(false);
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_setup_previews_imports_preserves_policy_and_verifies() {
    flow(true);
}

#[test]
fn setup_requires_explicit_apply_confirmation_and_preview_needs_no_database_credentials() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let before = files(dir.path());
    cli(dir.path())
        .args(["storage", "setup", "sqlite", "--apply"])
        .assert()
        .failure();
    cli(dir.path())
        .args(["storage", "setup", "sqlite", "--confirm-stopped"])
        .assert()
        .failure();
    cli(dir.path())
        .env_remove("JEVIA_MISSING_URL")
        .args([
            "storage",
            "setup",
            "postgres",
            "--url-env",
            "JEVIA_MISSING_URL",
            "--project",
            "test",
        ])
        .assert()
        .success();
    assert_eq!(files(dir.path()), before);
}

#[test]
fn fresh_setup_accepts_absolute_sqlite_paths_and_needs_no_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root);
    let path = root.join(".jevia/history with spaces.db");
    cli(root)
        .args([
            "storage",
            "setup",
            "sqlite",
            "--path",
            path.to_str().unwrap(),
            "--apply",
            "--confirm-stopped",
        ])
        .assert()
        .success();
    assert!(path.is_file());
    assert!(!root.join(".jevia/runs.jsonl").exists());
    assert_eq!(
        json_output(root, &["stats", "--json"])["totals"]["records"],
        0
    );
}

#[test]
fn failed_connection_or_bad_url_keeps_config_and_redacts_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    for url in [
        "not-a-url-secret",
        "postgres://user:secret@example.com/db?sslmode=disable",
        "postgres://user:secret@127.0.0.1:1/db",
    ] {
        let output = cli(root)
            .env("JEVIA_SETUP_URL", url)
            .args([
                "storage",
                "setup",
                "postgres",
                "--url-env",
                "JEVIA_SETUP_URL",
                "--project",
                "test",
                "--allow-insecure-localhost",
                "--apply",
                "--confirm-stopped",
            ])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert_eq!(
            fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
            original
        );
        assert!(!root.join(".jevia/runs.jsonl").exists());
    }
}

#[test]
fn invalid_active_or_duplicate_source_refuses_setup_without_creating_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    let mut active = record("active");
    active["lifecycle"] = json!({"state": "running", "started_at_ms": 1, "finished_at_ms": null});
    let mut unsupported = record("unsupported");
    unsupported["schema_version"] = json!(999);
    for contents in [
        "{broken".into(),
        format!("{active}\n"),
        format!("{unsupported}\n"),
        format!("{0}\n{0}\n", record("duplicate")),
    ] {
        fs::write(root.join(".jevia/runs.jsonl"), &contents).unwrap();
        cli(root)
            .args([
                "storage",
                "setup",
                "sqlite",
                "--import-jsonl",
                "--apply",
                "--confirm-stopped",
            ])
            .assert()
            .failure();
        assert_eq!(
            fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(root.join(".jevia/runs.jsonl")).unwrap(),
            contents
        );
        assert!(!root.join(".jevia/jevia.db").exists());
    }
}

#[test]
fn backup_failure_and_reserved_paths_do_not_switch_or_damage_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    for path in [
        ".jevia/config.toml",
        ".jevia/runs.jsonl",
        ".jevia/RUNS.JSONL",
        ".jevia/../.jevia/cache.jsonl",
        ".jevia/config.lock",
    ] {
        cli(root)
            .args([
                "storage",
                "setup",
                "sqlite",
                "--path",
                path,
                "--apply",
                "--confirm-stopped",
            ])
            .assert()
            .failure();
        assert_eq!(
            fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
            original
        );
        assert!(!root.join(".jevia/runs.jsonl").exists());
        assert!(!root.join(".jevia/cache.jsonl").exists());
    }
    fs::write(root.join(".jevia/config-backups"), "not a directory").unwrap();
    cli(root)
        .args(["storage", "setup", "sqlite", "--apply", "--confirm-stopped"])
        .assert()
        .failure();
    assert!(!root.join(".jevia/jevia.db").exists());
    assert_eq!(
        fs::read_to_string(root.join(".jevia/config.toml")).unwrap(),
        original
    );
}
