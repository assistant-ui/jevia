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

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_cli_flow_and_shared_evidence_cache_invalidation() {
    flow(true);
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
