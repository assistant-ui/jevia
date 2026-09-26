use std::fs;

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
