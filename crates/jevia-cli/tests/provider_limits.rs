use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use assert_cmd::Command;
use tempfile::tempdir;

const LIMIT: usize = 1024 * 1024;
const PRIVATE: &str = "PRIVATE_RESPONSE_SENTINEL";

#[derive(Clone, Copy)]
enum Framing {
    Length,
    Chunked,
    Close,
    HeaderOnly,
}

fn check(size: usize, framing: Framing, status: u16, success: bool) {
    let root = tempdir().unwrap();
    Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(root.path())
        .arg("init")
        .assert()
        .success();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = jevia_core::Config::default();
    config.jev.base_url = format!("http://{address}");
    config.jev.timeout_ms = 3000;
    fs::write(
        root.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
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
        reader.read_exact(&mut vec![0; length]).unwrap();
        let mut body = serde_json::json!({"model":"test", "answers":{"tier":{
            "type":"choice", "choice":"fast", "confidence":0.9
        }}, "padding":PRIVATE})
        .to_string();
        // JSON permits trailing whitespace; retain a sensitive sentinel in all cases.
        body.extend(std::iter::repeat_n(' ', size - body.len()));
        let header = match framing {
            Framing::Length | Framing::HeaderOnly => format!("Content-Length: {size}\r\n"),
            Framing::Chunked => "Transfer-Encoding: chunked\r\n".into(),
            Framing::Close => String::new(),
        };
        write!(
            socket,
            "HTTP/1.1 {status} Test\r\n{header}Connection: close\r\n\r\n"
        )
        .unwrap();
        // Early rejection is expected, so a broken pipe during body writes is fine.
        match framing {
            Framing::HeaderOnly => {
                let mut byte = [0];
                let _ = socket.read(&mut byte);
            }
            Framing::Chunked => {
                for chunk in body.as_bytes().chunks(8192) {
                    if write!(socket, "{:x}\r\n", chunk.len())
                        .and_then(|_| socket.write_all(chunk))
                        .and_then(|_| socket.write_all(b"\r\n"))
                        .is_err()
                    {
                        return;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n");
            }
            _ => {
                let _ = socket.write_all(body.as_bytes());
            }
        }
    });
    let output = Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(root.path())
        .env("TYPESAFE_API_KEY", PRIVATE)
        .timeout(Duration::from_secs(10))
        .args(["route", "fixture", "--json"])
        .output()
        .unwrap();
    server.join().unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(PRIVATE));
    if success {
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(record["tier"], "fast");
    } else {
        assert!(output.stdout.is_empty());
        assert!(stderr.contains(if status == 200 {
            "response exceeds 1 MiB limit"
        } else {
            "HTTP status 401"
        }));
        assert!(!root.path().join(".jevia/runs.jsonl").exists());
        assert!(!root.path().join(".jevia/cache.jsonl").exists());
    }
}

#[test]
fn provider_response_limits_cover_all_http_body_framings() {
    for framing in [Framing::Length, Framing::Chunked, Framing::Close] {
        for size in [LIMIT - 1, LIMIT, LIMIT + 1] {
            check(size, framing, 200, size <= LIMIT);
        }
    }
}

#[test]
fn oversized_headers_reject_without_waiting_for_body_and_preserve_http_errors() {
    check(LIMIT + 1, Framing::HeaderOnly, 200, false);
    check(LIMIT + 1, Framing::HeaderOnly, 401, false);
}
