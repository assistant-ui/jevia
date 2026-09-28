use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use tempfile::tempdir;

struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
    evidence_counts: Arc<Mutex<Vec<usize>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn new(fail_first: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let evidence_counts = Arc::new(Mutex::new(Vec::new()));
        let counts = Arc::clone(&evidence_counts);
        let stop = Arc::new(AtomicBool::new(false));
        let counter = Arc::clone(&requests);
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let index = counter.fetch_add(1, Ordering::SeqCst);
                        let counts = Arc::clone(&counts);
                        handlers.push(thread::spawn(move || {
                            respond(stream, fail_first && index == 0, &counts)
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
            evidence_counts,
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

fn respond(mut stream: TcpStream, fail: bool, counts: &Mutex<Vec<usize>>) {
    // Accepted sockets can inherit the listener's non-blocking mode on macOS
    // and Windows. Worker threads use blocking reads with a bounded timeout.
    stream.set_nonblocking(false).unwrap();
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
                let body: serde_json::Value =
                    serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                counts.lock().unwrap().push(
                    body["state"]["recent_completed_outcomes"]
                        .as_array()
                        .unwrap()
                        .len(),
                );
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
    route_with_options(root, no_cache, false)
}

fn route_with_options(root: &Path, no_cache: bool, explain: bool) -> Child {
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
    if explain {
        command.arg("--explain");
    }
    command.spawn().unwrap()
}

#[test]
fn concurrent_misses_share_one_request_and_keep_distinct_runs() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let children: Vec<_> = (0..6)
        .map(|_| route_with_options(root.path(), false, true))
        .collect();
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
        let explanation = String::from_utf8_lossy(&output.stderr);
        assert!(explanation.contains(if record["source"] == "cache" {
            "explain cache=hit"
        } else {
            "explain cache=miss:not_found"
        }));
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
fn cached_routes_preserve_a_history_without_a_final_newline() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    assert!(
        route(root.path(), false)
            .wait_with_output()
            .unwrap()
            .status
            .success()
    );
    let path = root.path().join(".jevia/runs.jsonl");
    let mut original = fs::read(&path).unwrap();
    assert_eq!(original.pop(), Some(b'\n'));
    fs::write(&path, &original).unwrap();

    // Exercise the boundary under competing appenders, including cache hits.
    let children: Vec<_> = (0..6).map(|_| route(root.path(), false)).collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(record["source"], "cache");
    }
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.starts_with(&original));
    let records: Vec<jevia_core::RouteRecord> = String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 7);
    assert_eq!(
        records
            .iter()
            .map(|r| &r.decision.run_id)
            .collect::<BTreeSet<_>>()
            .len(),
        7
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
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
    let output = route_with_options(root.path(), false, true)
        .wait_with_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("coordination wait expired"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("coordination=timed_out"));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

fn explained(root: &Path, bypass: bool) -> (serde_json::Value, String) {
    let output = route_with_options(root, bypass, true)
        .wait_with_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // stdout remains exactly the existing record, not an explanation envelope.
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    serde_json::from_value::<jevia_core::RouteRecord>(record.clone()).unwrap();
    assert!(record.get("explanation").is_none());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("jevia: explain "))
            .count(),
        3
    );
    assert!(!stderr.contains("same task"));
    assert!(!stderr.contains("test-key"));
    assert!(!stderr.contains(record["run_id"].as_str().unwrap()));
    (record, stderr)
}

fn edit_config(root: &Path, edit: impl FnOnce(&mut jevia_core::Config)) {
    let path = root.join(".jevia/config.toml");
    let mut config = jevia_core::Config::from_toml(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut config);
    fs::write(path, config.to_toml().unwrap()).unwrap();
}

#[test]
fn explanations_distinguish_miss_hit_expiry_bypass_and_disabled_without_changing_json() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let (record, first) = explained(root.path(), false);
    assert_eq!(record["source"], "live");
    assert!(first.contains("cache=miss:not_found coordination=acquired write=stored"));
    assert!(first.contains("eligible_evidence=0"));
    assert!(first.contains("fallback=not_applied"));
    let (_, hit) = explained(root.path(), false);
    assert!(hit.contains("cache=hit coordination=not_needed write=skipped"));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);

    let cache_path = root.path().join(".jevia/cache.jsonl");
    let mut entry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
    entry["expires_at_ms"] = 0.into();
    fs::write(&cache_path, format!("{entry}\n")).unwrap();
    assert!(
        explained(root.path(), false)
            .1
            .contains("cache=miss:expired")
    );
    let before = fs::read(&cache_path).unwrap();
    assert!(
        explained(root.path(), true)
            .1
            .contains("cache=bypassed coordination=not_needed write=skipped")
    );
    assert_eq!(fs::read(&cache_path).unwrap(), before);

    edit_config(root.path(), |config| config.cache.enabled = false);
    assert!(
        explained(root.path(), false)
            .1
            .contains("cache=disabled coordination=not_needed write=skipped")
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 4);
    let normal = route(root.path(), false).wait_with_output().unwrap();
    assert!(normal.status.success());
    assert!(!String::from_utf8_lossy(&normal.stderr).contains("jevia: explain"));
    assert_eq!(
        record.as_object().unwrap().keys().collect::<Vec<_>>(),
        serde_json::from_slice::<serde_json::Value>(&normal.stdout)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>()
    );
}

#[test]
fn explanations_count_only_request_evidence_and_report_confidence_fallback() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    edit_config(root.path(), |config| {
        config.router.confidence_floor = 0.99;
        config.router.history_limit = 2;
        config.privacy.store_task_text = false;
    });
    let (record, first) = explained(root.path(), false);
    assert_eq!(record["fallback_applied"], true);
    assert!(first.contains("confidence=0.94 floor=0.99 fallback=below_confidence_floor"));
    let mut history = Vec::new();
    for (index, (source, state, outcome)) in [
        ("manual", "completed", "success"),
        ("verification", "completed", "failure"),
        ("manual", "completed", "success"),
        ("process_exit", "completed", "success"),
        ("manual", "running", "success"),
        ("manual", "completed", "unknown"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut seed = record.clone();
        seed["run_id"] = format!("seed-{index}").into();
        seed["task"] = "PRIVATE_EVIDENCE_TASK".into();
        seed["outcome"] = outcome.into();
        seed["lifecycle"]["state"] = state.into();
        seed["outcome_evidence"] = serde_json::json!({"source": source, "recorded_at_ms": 1});
        history.push(seed.to_string());
    }
    fs::write(
        root.path().join(".jevia/runs.jsonl"),
        format!("{}\n", history.join("\n")),
    )
    .unwrap();
    let (_, explanation) = explained(root.path(), false);
    assert!(explanation.contains("eligible_evidence=2 history_limit=2"));
    assert!(!explanation.contains("PRIVATE_EVIDENCE_TASK"));
    assert_eq!(*server.evidence_counts.lock().unwrap(), [0, 2]);
    let (_, cached) = explained(root.path(), false);
    assert!(cached.contains("cache=hit"));
    assert!(cached.contains("eligible_evidence=2 history_limit=2"));
    assert!(cached.contains("fallback=below_confidence_floor"));
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn explanations_report_unavailable_cache_and_coordination_without_payloads() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    let path = root.path().join(".jevia/cache.jsonl");
    let malformed = br#"{"schema_version":"PRIVATE_CACHE_VALUE"}"#;
    fs::write(&path, malformed).unwrap();
    let (_, explanation) = explained(root.path(), false);
    assert!(explanation.contains("cache=unavailable coordination=not_needed write=skipped"));
    assert!(!explanation.contains("PRIVATE_CACHE_VALUE"));
    assert_eq!(fs::read(&path).unwrap(), malformed);

    // A regular file prevents creating the lease directory, but routing and
    // cache storage still work; the explanation must reflect the degraded path.
    fs::write(&path, "").unwrap();
    fs::write(root.path().join(".jevia/cache-leases"), "not a directory").unwrap();
    assert!(
        explained(root.path(), false)
            .1
            .contains("coordination=unavailable write=stored")
    );
}

#[cfg(unix)]
#[test]
fn run_explanation_is_opt_in_and_precedes_the_harness() {
    let root = tempdir().unwrap();
    let server = Server::new(false);
    server.configure(root.path());
    edit_config(root.path(), |config| {
        config.harnesses.insert(
            "test-harness".into(),
            jevia_core::HarnessConfig {
                command: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "printf HARNESS_STARTED >&2".into(),
                    "fixture".into(),
                    "{task}".into(),
                    "{model}".into(),
                ],
                models: config
                    .tiers
                    .keys()
                    .map(|tier| (tier.clone(), "private-model".into()))
                    .collect(),
                verification: None,
            },
        );
    });
    let config = jevia_core::Config::from_toml(
        &fs::read_to_string(root.path().join(".jevia/config.toml")).unwrap(),
    )
    .unwrap();
    let name = config.harnesses.keys().next().unwrap();
    for explain in [true, false] {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin("jevia"));
        command
            .current_dir(root.path())
            .env("TYPESAFE_API_KEY", "test-key")
            .env("TOKIO_WORKER_THREADS", "2")
            .args(["run", name, "PRIVATE_RUN_TASK", "--non-interactive"]);
        if explain {
            command.arg("--explain");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("PRIVATE_RUN_TASK"));
        assert_eq!(stderr.contains("jevia: explain"), explain);
        if explain {
            assert!(
                stderr.find("jevia: explain").unwrap() < stderr.find("HARNESS_STARTED").unwrap()
            );
        }
    }
}

#[test]
fn failed_routing_does_not_print_a_success_explanation_or_store_a_record() {
    let root = tempdir().unwrap();
    let server = Server::new(true);
    server.configure(root.path());
    let output = route_with_options(root.path(), false, true)
        .wait_with_output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("jevia: explain"));
    assert!(!root.path().join(".jevia/runs.jsonl").exists());
}
