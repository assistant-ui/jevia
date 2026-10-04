use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use assert_cmd::Command;
use jevia_core::{Config, RouteRecord, StorageConfig};
use serde_json::{Value, json};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env("TYPESAFE_API_KEY", "synthetic-loopback-only")
        .env("TOKIO_WORKER_THREADS", "2")
        .timeout(Duration::from_secs(20));
    command
}

fn contract(storage: StorageConfig) {
    let root = tempfile::tempdir().unwrap();
    cli(root.path()).arg("init").assert().success();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut config = Config {
        storage,
        ..Config::default()
    };
    config.jev.base_url = format!("http://{}", listener.local_addr().unwrap());
    config.jev.timeout_ms = 5000;
    config.privacy.store_task_text = true;
    fs::write(
        root.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    let mut originals = Vec::new();
    for i in 0..20 {
        for kind in ["known", "passive"] {
            let mut row = json!({
                "schema_version":6, "run_id":format!("fixture-{kind}-{i:02}"),
                "tier":"fast", "suggested_tier":"fast", "confidence":0.9,
                "probabilities":{}, "fallback_applied":false, "jev_model":"fixture",
                "created_at_ms":i, "source":"live", "outcome":"unknown",
                "task":format!("{kind}-{i:02}:{}", if kind == "known" { "x".repeat(64 * 1024) } else { "\0".repeat(4096) }),
                "lifecycle":{"state":"completed", "started_at_ms":1, "finished_at_ms":2},
                "execution":{"harness":"fixture", "model":"requested", "duration_ms":1, "exit_code":0}
            });
            if kind == "known" {
                row["outcome"] = "success".into();
                row["outcome_evidence"] = json!({"source":"manual", "recorded_at_ms":2});
            }
            // Use the canonical stored representation, including serde defaults.
            let typed: RouteRecord = serde_json::from_value(row).unwrap();
            originals.push(serde_json::to_value(typed).unwrap());
        }
    }
    let original: String = originals.iter().map(|r| format!("{r}\n")).collect();
    let history = root.path().join(".jevia/runs.jsonl");
    fs::write(&history, &original).unwrap();
    if !config.storage.is_jsonl() {
        cli(root.path())
            .args(["storage", "init"])
            .assert()
            .success();
        cli(root.path())
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
    }
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("mock provider accept failed: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(&mut socket);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
        }
        assert!(length < 140 * 1024, "outbound request was {length} bytes");
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        let body = r#"{"model":"fixture","answers":{"tier":{"type":"choice","choice":"fast","confidence":0.9}}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    let output = cli(root.path())
        .args(["route", "small task", "--json", "--explain"])
        .assert()
        .success()
        .get_output()
        .clone();
    let request = server.join().unwrap();
    assert_eq!(request["state"]["current_task"], "small task");
    for (field, kind) in [
        ("recent_completed_outcomes", "known_outcomes"),
        ("recent_execution_observations", "passive_observations"),
    ] {
        let records = request["state"][field].as_array().unwrap();
        let usage = &request["state"]["history_budget"][kind];
        let bytes = serde_json::to_vec(records).unwrap().len();
        assert!(bytes <= 64 * 1024);
        assert_eq!(usage["serialized_bytes"], bytes);
        assert_eq!(usage["included"], records.len());
        assert_eq!(usage["candidates"], 20);
        assert!(!records.is_empty());
        assert!(
            records
                .iter()
                .all(|r| r["task"].as_str().unwrap().len() <= 2048 && r["task_truncated"] == true)
        );
        assert!(
            records.last().unwrap()["task"]
                .as_str()
                .unwrap()
                .starts_with(if kind == "known_outcomes" {
                    "known-19:"
                } else {
                    "passive-19:"
                })
        );
    }
    assert_eq!(
        request["state"]["history_budget"]["known_outcomes"]["included"],
        20
    );
    assert!(
        request["state"]["history_budget"]["passive_observations"]["omitted"]
            .as_u64()
            .unwrap()
            > 0
    );
    let explanation = String::from_utf8(output.stderr).unwrap();
    assert!(explanation.contains("known_outcomes=20 passive_observations=20"));
    assert!(explanation.contains("history_stage=candidates_before_byte_budget"));
    // The server has closed: equivalent projected context must hit cache.
    let cached = cli(root.path())
        .args(["route", "small task", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&cached).unwrap()["source"],
        "cache"
    );
    let saved = cli(root.path())
        .args(["runs", "--limit", "100", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let saved: Vec<Value> = serde_json::from_slice(&saved).unwrap();
    for original in &originals {
        assert_eq!(
            saved
                .iter()
                .find(|r| r["run_id"] == original["run_id"])
                .unwrap(),
            original
        );
    }
    assert!(fs::read_to_string(history).unwrap().starts_with(&original));
}

#[test]
fn jsonl_and_sqlite_route_with_bounded_history_without_rewriting_records() {
    contract(StorageConfig::Jsonl);
    contract(StorageConfig::Sqlite {
        url: "sqlite://.jevia/runs.db".into(),
    });
}

#[test]
#[ignore = "requires JEVIA_TEST_POSTGRES_URL"]
fn postgres_routes_with_bounded_history_without_rewriting_records() {
    contract(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: format!("history-budget-{}", uuid::Uuid::new_v4()),
        allow_insecure_localhost: true,
    });
}
