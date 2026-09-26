use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use tempfile::tempdir;

struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn new(fail_first: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let counter = Arc::clone(&requests);
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let index = counter.fetch_add(1, Ordering::SeqCst);
                        handlers.push(thread::spawn(move || {
                            respond(stream, fail_first && index == 0)
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn configure(&self, root: &Path) {
        let mut config = jevia_core::Config::default();
        config.jev.base_url = self.url.clone();
        config.jev.timeout_ms = 5_000;
        fs::create_dir_all(root.join(".jevia")).unwrap();
        fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Err(error) = self.thread.take().unwrap().join() {
            // Preserve the original assertion failure instead of aborting from
            // a second panic while unwinding (especially on Windows).
            if thread::panicking() {
                eprintln!("mock server also failed: {error:?}");
            } else {
                std::panic::resume_unwind(error);
            }
        }
    }
}

fn respond(mut stream: TcpStream, fail: bool) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let size = stream.read(&mut buffer).unwrap();
        assert_ne!(size, 0);
        request.extend_from_slice(&buffer[..size]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
    thread::sleep(Duration::from_millis(250));
    let status = if fail {
        "500 Internal Server Error"
    } else {
        "200 OK"
    };
    let body = r#"{"model":"jev-test","answers":{"tier":{"type":"choice","choice":"fast","confidence":0.94,"probabilities":{"fast":0.94,"balanced":0.05,"strong":0.01}}}}"#;
    write!(stream, "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn route(root: &Path, no_cache: bool) -> Child {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("jevia"));
    command
        .current_dir(root)
        .env("TYPESAFE_API_KEY", "test-key")
        // These tests launch several independent runtimes concurrently. Avoid
        // multiplying the CI host's CPU count into hundreds of worker threads.
        .env("TOKIO_WORKER_THREADS", "2")
        .args(["route", "same task", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if no_cache {
        command.arg("--no-cache");
    }
    command.spawn().unwrap()
}

#[test]
fn concurrent_misses_share_one_request_and_keep_distinct_runs() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let children: Vec<_> = (0..6).map(|_| route(root.path(), false)).collect();
    let mut ids = BTreeSet::new();
    let mut live = 0;
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        ids.insert(record["run_id"].as_str().unwrap().to_owned());
        live += usize::from(record["source"] == "live");
    }
    assert_eq!(ids.len(), 6);
    assert_eq!(live, 1);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(root.path().join(".jevia/runs.jsonl"))
            .unwrap()
            .lines()
            .count(),
        6
    );
}

#[test]
fn failed_leader_releases_lease_and_is_not_cached() {
    let root = tempdir().unwrap();
    let server = Server::new(true);
    server.configure(root.path());
    let first = route(root.path(), false);
    let second = route(root.path(), false);
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    assert!(
        route(root.path(), false)
            .wait_with_output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn forced_live_requests_bypass_cache_and_coordination() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let children = [route(root.path(), true), route(root.path(), true)];
    for child in children {
        assert!(child.wait_with_output().unwrap().status.success());
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    assert!(!root.path().join(".jevia/cache-leases").exists());
    assert!(!root.path().join(".jevia/cache.jsonl").exists());
}

#[test]
fn busy_lease_has_a_bounded_wait_and_falls_back_to_live() {
    use sha2::{Digest, Sha256};
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let path = root.path().join(".jevia/config.toml");
    let mut config = jevia_core::Config::from_toml(&fs::read_to_string(&path).unwrap()).unwrap();
    config.jev.timeout_ms = 1_000;
    fs::write(path, config.to_toml().unwrap()).unwrap();
    let key = jevia_core::route_cache_key("same task", None, &config, &[]).unwrap();
    let name = Sha256::digest(&key.as_bytes()[..2])
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let leases = root.path().join(".jevia/cache-leases");
    fs::create_dir_all(&leases).unwrap();
    let guard = fs::File::create(leases.join(format!("{name}.lock"))).unwrap();
    guard.lock().unwrap();
    let output = route(root.path(), false).wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("coordination wait expired"));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}
