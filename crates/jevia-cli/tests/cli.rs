use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn help_is_available() {
    let mut command = Command::cargo_bin("jevia").expect("binary is built");
    command.arg("--help").assert().success();
}

#[test]
fn init_creates_a_valid_project_configuration() {
    let directory = tempdir().expect("temporary directory");
    let mut command = Command::cargo_bin("jevia").expect("binary is built");
    command
        .current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let config_path = directory.path().join(".jevia/config.toml");
    let config = fs::read_to_string(config_path).expect("config is readable");
    let parsed = jevia_core::Config::from_toml(&config).expect("config is valid");

    assert_eq!(parsed.router.fallback_tier, "strong");
    assert!(directory.path().join(".jevia/.gitignore").is_file());
    let ignore = fs::read_to_string(directory.path().join(".jevia/.gitignore"))
        .expect("ignore file is readable");
    assert!(ignore.lines().any(|line| line == "runs.lock"));
    assert!(ignore.lines().any(|line| line == "cache.jsonl"));
    assert!(ignore.lines().any(|line| line == "cache.lock"));
}

#[test]
fn init_does_not_overwrite_configuration_without_force() {
    let directory = tempdir().expect("temporary directory");
    let mut first = Command::cargo_bin("jevia").expect("binary is built");
    first
        .current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let mut second = Command::cargo_bin("jevia").expect("binary is built");
    second
        .current_dir(directory.path())
        .arg("init")
        .assert()
        .failure();
}

#[test]
fn init_preserves_existing_ignore_rules() {
    let directory = tempdir().expect("temporary directory");
    let jevia_directory = directory.path().join(".jevia");
    fs::create_dir_all(&jevia_directory).expect("Jevia directory is created");
    fs::write(jevia_directory.join(".gitignore"), "custom-rule\n")
        .expect("custom ignore file is written");

    let mut command = Command::cargo_bin("jevia").expect("binary is built");
    command
        .current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let ignore =
        fs::read_to_string(jevia_directory.join(".gitignore")).expect("ignore file is readable");
    assert!(ignore.lines().any(|line| line == "custom-rule"));
    assert!(ignore.lines().any(|line| line == "cache.jsonl"));
}

#[test]
fn cache_commands_are_available() {
    let directory = tempdir().expect("temporary directory");
    let mut init = Command::cargo_bin("jevia").expect("binary is built");
    init.current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let mut status = Command::cargo_bin("jevia").expect("binary is built");
    status
        .current_dir(directory.path())
        .args(["cache", "status"])
        .assert()
        .success();

    let mut clear = Command::cargo_bin("jevia").expect("binary is built");
    clear
        .current_dir(directory.path())
        .args(["cache", "clear"])
        .assert()
        .success();
}

#[test]
fn check_completes_a_live_round_trip_without_storing_a_run() {
    let directory = tempdir().expect("temporary directory");
    let jevia_directory = directory.path().join(".jevia");
    fs::create_dir_all(&jevia_directory).expect("Jevia directory is created");

    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test request arrives");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("test request is readable");
        let body = r#"{"model":"jev-test","answers":{"tier":{"type":"choice","choice":"fast","confidence":0.94,"probabilities":{"fast":0.94,"balanced":0.05,"strong":0.01}}}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("test response is written");
    });

    let mut config = jevia_core::Config::default();
    config.jev.base_url = format!("http://{address}");
    fs::write(
        jevia_directory.join("config.toml"),
        config.to_toml().expect("config serializes"),
    )
    .expect("config is written");

    let mut check = Command::cargo_bin("jevia").expect("binary is built");
    let output = check
        .current_dir(directory.path())
        .env("TYPESAFE_API_KEY", "test-key")
        .arg("check")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).expect("stdout is UTF-8");

    server.join().expect("test server exits");
    assert!(output.contains("jev api: ok (model=jev-test, tier=fast, confidence=0.94)"));
    assert!(output.contains("jevia: ready"));
    assert!(!jevia_directory.join("runs.jsonl").exists());
}

#[test]
fn run_reports_when_a_harness_is_not_configured() {
    let directory = tempdir().expect("temporary directory");
    let mut init = Command::cargo_bin("jevia").expect("binary is built");
    init.current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let mut run = Command::cargo_bin("jevia").expect("binary is built");
    let output = run
        .current_dir(directory.path())
        .args(["run", "missing", "test task"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let error = String::from_utf8(output).expect("stderr is UTF-8");

    assert!(error.contains("harness `missing` is not configured"));
    assert!(error.contains("available harnesses: none"));
}

#[test]
fn checked_in_harness_example_remains_valid() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/jevia.toml");
    let input = fs::read_to_string(example).expect("example config is readable");
    let config = jevia_core::Config::from_toml(&input).expect("example config is valid");

    let harness = config
        .harnesses
        .get("agent")
        .expect("example defines the agent harness");
    assert_eq!(harness.command, "my-agent");
    assert_eq!(harness.models.len(), config.tiers.len());
    assert_eq!(
        harness
            .verification
            .as_ref()
            .expect("example defines verification")
            .command,
        "cargo"
    );
}

#[test]
fn doctor_reports_malformed_history_without_rewriting_it() {
    let directory = tempdir().expect("temporary directory");
    let mut init = Command::cargo_bin("jevia").expect("binary is built");
    init.current_dir(directory.path())
        .arg("init")
        .assert()
        .success();
    let history = directory.path().join(".jevia/runs.jsonl");
    let malformed = "{not valid json}\n";
    fs::write(&history, malformed).expect("malformed history is written");

    let mut doctor = Command::cargo_bin("jevia").expect("binary is built");
    let output = doctor
        .current_dir(directory.path())
        .env("TYPESAFE_API_KEY", "test-key")
        .arg("doctor")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let error = String::from_utf8(output).expect("stderr is UTF-8");

    assert!(error.contains("invalid run record on line 1"));
    assert_eq!(
        fs::read_to_string(history).expect("history remains readable"),
        malformed
    );
}
