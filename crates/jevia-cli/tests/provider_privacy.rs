use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use assert_cmd::Command;
use tempfile::tempdir;

const PRIVATE: &str = "PRIVATE_PROVIDER_SENTINEL";

fn route(endpoint: &str) -> String {
    let dir = tempdir().unwrap();
    Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();
    let path = dir.path().join(".jevia/config.toml");
    let mut config = jevia_core::Config::default();
    config.jev.base_url = endpoint.to_owned();
    config.jev.timeout_ms = 1000;
    fs::write(&path, config.to_toml().unwrap()).unwrap();
    let output = Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(dir.path())
        .env("TYPESAFE_API_KEY", PRIVATE)
        .timeout(Duration::from_secs(10))
        .args(["route", PRIVATE, "--json"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!text.contains(PRIVATE), "private diagnostic: {text}");
    text
}

#[test]
fn network_errors_do_not_echo_url_credentials_or_queries() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let text = route(&format!("http://user:{PRIVATE}@{address}/?token={PRIVATE}"));
    assert!(text.contains("Jev request failed"));
}

#[test]
fn provider_errors_do_not_echo_response_values_or_bodies() {
    let valid = serde_json::json!({"model": "test", "answers": {"tier": {
        "type": "choice", "choice": "fast", "confidence": 0.9
    }}});
    let mut cases = Vec::new();
    for field in ["type", "choice", "confidence", "probabilities"] {
        let mut invalid = valid.clone();
        invalid["answers"]["tier"][field] = PRIVATE.into();
        cases.push((200, invalid.to_string()));
    }
    cases.push((200, PRIVATE.to_owned()));
    cases.push((401, PRIVATE.to_owned()));
    for (status, body) in cases {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
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
            reader.read_exact(&mut vec![0; length]).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let text = route(&format!("http://{address}"));
        server.join().unwrap();
        if status == 401 {
            assert!(text.contains("HTTP status 401"));
        } else {
            assert!(text.contains("Jev returned") || text.contains("Jev selected"));
        }
    }
}
