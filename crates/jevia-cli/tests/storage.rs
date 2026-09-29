use assert_cmd::Command;
use jevia_core::{Config, HarnessConfig, StorageConfig, VerificationConfig};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn command(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env("TYPESAFE_API_KEY", "test-only")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn json(root: &Path, args: &[&str]) -> Value {
    let bytes = command(root)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&bytes).unwrap()
}

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let captured = Arc::clone(&requests);
        let stop = Arc::clone(&stopped);
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(socket) => socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let size = socket.read(&mut buffer).unwrap();
                    assert_ne!(size, 0);
                    bytes.extend_from_slice(&buffer[..size]);
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            captured.lock().unwrap().push(
                                serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap(),
                            );
                            break;
                        }
                    }
                }
                let body = r#"{"model":"jev-test","answers":{"tier":{"type":"choice","choice":"fast","confidence":0.94,"probabilities":{"fast":0.94,"balanced":0.05,"strong":0.01}}}}"#;
                write!(socket, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn flow(postgres: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    command(root).arg("init").assert().success();
    let server = Server::start();
    let storage = if postgres {
        StorageConfig::Postgres {
            url_env: "JEVIA_TEST_POSTGRES_URL".into(),
            project: format!("cli-{}", uuid::Uuid::new_v4()),
            allow_insecure_localhost: true,
        }
    } else {
        StorageConfig::Sqlite {
            url: "sqlite://.jevia/jevia.db".into(),
        }
    };
    let mut config = Config {
        storage,
        ..Config::default()
    };
    config.jev.base_url = server.url.clone();
    config.privacy.store_task_text = false;
    config.harnesses.insert(
        "test".into(),
        HarnessConfig {
            auto_verify: true,
            command: "rustc".into(),
            args: vec![
                "--version".into(),
                "--cfg".into(),
                "task=\"{task}\"".into(),
                "--cfg".into(),
                "model=\"{model}\"".into(),
            ],
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "test/model".into()))
                .collect(),
            verification: Some(VerificationConfig {
                command: "rustc".into(),
                args: vec!["--version".into()],
            }),
        },
    );
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    command(root).args(["storage", "init"]).assert().success();
    command(root)
        .env_remove("TYPESAFE_API_KEY")
        .args(["storage", "check"])
        .assert()
        .success();
    let subdir = root.join("subdir");
    fs::create_dir(&subdir).unwrap();
    let first = json(&subdir, &["route", "private task", "--json"]);
    let second = json(root, &["route", "private task", "--json"]);
    assert_eq!(second["source"], "cache");
    assert_ne!(first["run_id"], second["run_id"]);
    assert!(first["task"].is_null());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    // A second project directory simulates another CLI machine/workspace. SQL
    // history is shared; each workspace still has its own decision cache.
    let other = tempfile::tempdir().unwrap();
    command(other.path()).arg("init").assert().success();
    let mut other_config = config.clone();
    if !postgres {
        other_config.storage = StorageConfig::Sqlite {
            url: format!("sqlite://{}", root.join(".jevia/jevia.db").display()),
        };
    }
    fs::write(
        other.path().join(".jevia/config.toml"),
        other_config.to_toml().unwrap(),
    )
    .unwrap();
    command(other.path())
        .args(["feedback", first["run_id"].as_str().unwrap(), "success"])
        .assert()
        .success();
    let third = json(root, &["route", "private task", "--json"]);
    assert_eq!(third["source"], "live");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["state"]["recent_completed_outcomes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(requests[1]["state"]["recent_completed_outcomes"][0]["task"].is_null());
    drop(requests);
    command(root)
        .args(["run", "test", "private task", "--non-interactive"])
        .assert()
        .success();
    let last = json(root, &["runs", "--limit", "1", "--json"]);
    assert_eq!(last[0]["lifecycle"]["state"], "completed");
    assert_eq!(last[0]["outcome_evidence"]["source"], "verification");
    command(root).args(["doctor"]).assert().success();
    command(root).args(["check"]).assert().success();
    assert_eq!(json(root, &["runs", "--json"]).as_array().unwrap().len(), 4);
    command(root)
        .args(["storage", "export", "--output", ".jevia/export.jsonl"])
        .assert()
        .success();
    command(root)
        .args([
            "storage",
            "import-jsonl",
            "--from",
            ".jevia/export.jsonl",
            "--apply",
        ])
        .assert()
        .success();
    command(root).args(["runs", "repair"]).assert().failure();
    assert!(!root.join(".jevia/runs.jsonl").exists());
}

#[test]
fn sqlite_cli_flow_and_shared_evidence_cache_invalidation() {
    flow(false);
}

fn automatic_rust_flow(storage: StorageConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    command(root).arg("init").assert().success();
    let server = Server::start();
    let mut config = Config {
        storage,
        ..Config::default()
    };
    config.jev.base_url = server.url.clone();
    config.harnesses.insert(
        "agent".into(),
        HarnessConfig {
            command: "rustc".into(),
            args: vec![
                "--version".into(),
                "--cfg".into(),
                "task=\"{task}\"".into(),
                "--cfg".into(),
                "model=\"{model}\"".into(),
            ],
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "test".into()))
                .collect(),
            auto_verify: true,
            verification: None,
        },
    );
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    if !matches!(config.storage, StorageConfig::Jsonl) {
        command(root).args(["storage", "init"]).assert().success();
    }
    fs::write(root.join("Cargo.toml"), "[package]\nname='verification-fixture'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n").unwrap();
    fs::write(
        root.join("lib.rs"),
        "#[test] fn verified() { assert_eq!(std::env::var(\"CI\").unwrap(), \"true\"); }",
    )
    .unwrap();
    command(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", root.join("build"))
        .args(["run", "agent", "first task"])
        .assert()
        .success();
    let first = json(root, &["runs", "--limit", "1", "--json"]);
    assert_eq!(first[0]["lifecycle"]["state"], "completed");
    assert_eq!(first[0]["outcome"], "success");
    assert_eq!(first[0]["outcome_evidence"]["source"], "verification");
    assert_eq!(first[0]["execution"]["verification"]["command"], "cargo");
    assert!(first[0].get("feedback").is_none());
    fs::write(
        root.join("lib.rs"),
        "#[test] fn regression() { panic!(\"test failed\"); }",
    )
    .unwrap();
    command(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", root.join("build"))
        .args(["run", "agent", "second task"])
        .assert()
        .failure();
    let second = json(root, &["runs", "--limit", "1", "--json"]);
    assert_eq!(second[0]["outcome"], "failure");
    assert_eq!(second[0]["lifecycle"]["state"], "completed");
    assert_eq!(second[0]["outcome_evidence"]["source"], "verification");
    assert_eq!(
        server.requests.lock().unwrap()[1]["state"]["recent_completed_outcomes"][0]["outcome_source"],
        "verification"
    );
}

#[test]
fn automatic_rust_verification_records_and_learns_without_manual_feedback() {
    automatic_rust_flow(StorageConfig::Jsonl);
    automatic_rust_flow(StorageConfig::Sqlite {
        url: "sqlite://.jevia/auto.db".into(),
    });
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_automatic_rust_verification_records_and_learns() {
    automatic_rust_flow(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: format!("auto-{}", uuid::Uuid::new_v4()),
        allow_insecure_localhost: true,
    });
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_cli_flow_and_shared_evidence_cache_invalidation() {
    flow(true);
}

fn archive_flow(postgres: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    command(root).arg("init").assert().success();
    let server = Server::start();
    let mut config = Config {
        storage: if postgres {
            StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("archive-cli-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            }
        } else {
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/archive.db".into(),
            }
        },
        ..Config::default()
    };
    config.jev.base_url = server.url.clone();
    let state = root.join(".jevia");
    fs::write(state.join("config.toml"), config.to_toml().unwrap()).unwrap();
    command(root).args(["storage", "init"]).assert().success();
    let records: Vec<_> = (0..2).map(|index| serde_json::json!({
        "schema_version": 3, "run_id": format!("archive-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 2 - index, "task": "private-archive-task", "outcome": "success",
        "outcome_evidence": {"source": "manual", "recorded_at_ms": 3},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "feedback": [{"previous_outcome": "unknown", "previous_source": null, "outcome": "success", "recorded_at_ms": 3, "reason": "private-reason"}]
    })).collect();
    let source: String = records.iter().map(|record| format!("{record}\n")).collect();
    fs::write(state.join("runs.jsonl"), source).unwrap();
    command(root)
        .args(["storage", "import-jsonl", "--apply"])
        .assert()
        .success();
    // A stale JSONL file must not be read, repaired or used as a SQL fallback.
    fs::write(state.join("runs.jsonl"), "stale-private-not-json").unwrap();
    let first = json(root, &["route", "archive task", "--json"]);
    assert_eq!(first["source"], "live");
    assert_eq!(
        json(root, &["route", "archive task", "--json"])["source"],
        "cache"
    );
    let cache = fs::read(state.join("cache.jsonl")).unwrap();
    let before = json(root, &["stats", "--json"]);
    assert_eq!(before["totals"]["records"], 4);
    assert_eq!(before["totals"]["learning_evidence"], 2);
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let archive = |apply: bool| -> Value {
        let mut cli = command(&nested);
        cli.env_remove("TYPESAFE_API_KEY")
            .args(["runs", "archive", "--keep", "1", "--json"]);
        if apply {
            cli.arg("--apply");
        }
        let output = cli.assert().success().get_output().stdout.clone();
        assert!(!String::from_utf8_lossy(&output).contains("private-"));
        serde_json::from_slice(&output).unwrap()
    };
    let preview = archive(false);
    assert_eq!(preview["archived_records"], 1);
    assert_eq!(preview["retained_records"], 3);
    assert_eq!(preview["applied"], false);
    assert!(preview["backup"].is_null() && preview["archive"].is_null());
    assert!(!state.join("history-backups").exists());
    assert!(!state.join("history-archives").exists());
    assert_eq!(json(root, &["stats", "--json"]), before);
    let applied = archive(true);
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["archived_records"], 1);
    assert_eq!(applied["retained_records"], 3);
    let archive_path = Path::new(applied["archive"].as_str().unwrap());
    assert_eq!(
        fs::canonicalize(archive_path.parent().unwrap()).unwrap(),
        fs::canonicalize(state.join("history-archives")).unwrap()
    );
    let saved = fs::read(archive_path).unwrap();
    let archived: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(archived["run_id"], "archive-0");
    assert_eq!(archived["feedback"], records[0]["feedback"]);
    assert_eq!(
        fs::read_to_string(applied["backup"].as_str().unwrap())
            .unwrap()
            .lines()
            .count(),
        4
    );
    assert_eq!(fs::read(state.join("cache.jsonl")).unwrap(), cache);
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        "stale-private-not-json"
    );
    assert!(!nested.join(".jevia").exists());
    let after = json(root, &["stats", "--json"]);
    assert_eq!(after["totals"]["records"], 3);
    assert_eq!(after["totals"]["learning_evidence"], 1);
    assert_eq!(archive(true)["would_change"], false);
    assert_eq!(
        fs::read_dir(state.join("history-archives"))
            .unwrap()
            .count(),
        1
    );
    command(root)
        .args(["runs", "archive", "--keep", "0", "--apply"])
        .assert()
        .failure();
    command(root)
        .args(["runs", "repair", "--apply"])
        .assert()
        .failure();
    command(root)
        .args(["runs", "show", "archive-0"])
        .assert()
        .failure();
    command(root)
        .args(["feedback", "archive-0", "failure"])
        .assert()
        .failure();
    // Current SQL evidence is fetched before cache lookup, so the old decision is not reused.
    assert_eq!(
        json(root, &["route", "archive task", "--json"])["source"],
        "live"
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0]["state"]["recent_completed_outcomes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        requests[1]["state"]["recent_completed_outcomes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(requests);
    // Restore is explicit, preview-first, and appends old records as new evidence.
    command(&nested)
        .env_remove("TYPESAFE_API_KEY")
        .args(["storage", "import-jsonl", "--from"])
        .arg(archive_path)
        .assert()
        .success();
    command(root)
        .args(["runs", "show", "archive-0"])
        .assert()
        .failure();
    for _ in 0..2 {
        command(&nested)
            .env_remove("TYPESAFE_API_KEY")
            .args(["storage", "import-jsonl", "--from"])
            .arg(archive_path)
            .arg("--apply")
            .assert()
            .success();
    }
    let restored = json(root, &["runs", "show", "archive-0", "--json"]);
    assert_eq!(restored, archived);
    assert_eq!(
        json(root, &["runs", "--limit", "1", "--json"])[0]["run_id"],
        "archive-0"
    );
    assert_eq!(fs::read(archive_path).unwrap(), saved);
    assert_eq!(json(root, &["stats", "--json"])["totals"]["records"], 5);
    let ignore = fs::read_to_string(state.join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|line| line == "history-backups/"));
    assert!(ignore.lines().any(|line| line == "history-archives/"));
}

#[test]
fn sqlite_archive_cli_restore_stats_and_evidence_cache_invalidation() {
    archive_flow(false);
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_archive_cli_restore_stats_and_evidence_cache_invalidation() {
    archive_flow(true);
}

#[test]
fn invalid_database_url_fails_closed_without_leaking_secrets_or_creating_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    command(dir.path()).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Postgres {
            url_env: "JEVIA_BAD_URL".into(),
            project: "test".into(),
            allow_insecure_localhost: false,
        },
        ..Config::default()
    };
    fs::write(
        dir.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    for url in [
        "not-a-url-secret",
        "postgres://user:secret@example.com/db?sslmode=disable",
    ] {
        let output = command(dir.path())
            .env("JEVIA_BAD_URL", url)
            .args(["route", "test"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
        assert!(!dir.path().join(".jevia/runs.jsonl").exists());
    }
}
