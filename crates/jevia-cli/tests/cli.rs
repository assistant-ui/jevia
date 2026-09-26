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
