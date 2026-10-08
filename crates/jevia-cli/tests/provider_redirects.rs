use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use assert_cmd::Command;
use jevia_core::Config;
use serde_json::json;

#[test]
fn redirects_never_forward_tasks_history_or_credentials_or_populate_cache() {
    for status in [301, 302, 303, 307, 308] {
        for same_origin in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let cli = || {
                let mut command = Command::cargo_bin("jevia").unwrap();
                command
                    .current_dir(root.path())
                    .env("TOKIO_WORKER_THREADS", "2")
                    .env("TYPESAFE_API_KEY", "PRIVATE-key")
                    .timeout(Duration::from_secs(10));
                command
            };
            cli().arg("init").assert().success();
            let source = TcpListener::bind("127.0.0.1:0").unwrap();
            let destination = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = source.local_addr().unwrap();
            let target = if same_origin {
                address
            } else {
                destination.local_addr().unwrap()
            };
            let mut config = Config::default();
            config.jev.base_url = format!("http://{address}");
            config.jev.timeout_ms = 3000;
            fs::write(
                root.path().join(".jevia/config.toml"),
                config.to_toml().unwrap(),
            )
            .unwrap();
            let history = format!(
                "{}\n",
                json!({
                    "schema_version":6,"run_id":"old","tier":"fast","suggested_tier":"fast",
                    "confidence":0.9,"probabilities":{},"fallback_applied":false,"jev_model":"fixture",
                    "created_at_ms":1,"task":"PRIVATE-history","outcome":"success",
                    "outcome_evidence":{"source":"manual","recorded_at_ms":1}
                })
            );
            let path = root.path().join(".jevia/runs.jsonl");
            fs::write(&path, &history).unwrap();
            let server = thread::spawn(move || {
                let (mut stream, _) = source.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
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
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["state"]["current_task"], "PRIVATE-task");
                assert_eq!(
                    body["state"]["recent_completed_outcomes"][0]["task"],
                    "PRIVATE-history"
                );
                write!(stream, "HTTP/1.1 {status} Redirect\r\nLocation: http://{target}/PRIVATE-location\r\nContent-Length: 12\r\nConnection: close\r\n\r\nPRIVATE-body").unwrap();
                source
            });
            let output = cli()
                .args(["route", "PRIVATE-task", "--json"])
                .assert()
                .failure()
                .get_output()
                .clone();
            let source = server.join().unwrap();
            // The CLI has finished: a followed redirect would leave an accepted
            // or queued connection. No sleep/negative timing assertion is needed.
            for listener in [source, destination] {
                listener.set_nonblocking(true).unwrap();
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(&format!("HTTP status {status}")));
            assert!(!stderr.contains("PRIVATE"));
            assert!(output.stdout.is_empty());
            assert_eq!(fs::read_to_string(path).unwrap(), history);
            assert!(!root.path().join(".jevia/cache.jsonl").exists());
        }
    }
}
