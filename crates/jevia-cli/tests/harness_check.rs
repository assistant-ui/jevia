use assert_cmd::Command;
use jevia_core::{Config, HarnessConfig, StorageConfig, VerificationConfig};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("JEVIA_DATABASE_URL")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join(".jevia"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn fixture(root: &Path) -> Config {
    cli(root).arg("init").assert().success();
    let mut config = Config {
        storage: StorageConfig::Postgres {
            url_env: "JEVIA_DATABASE_URL".into(),
            project: "offline-preflight".into(),
            allow_insecure_localhost: false,
        },
        ..Config::default()
    };
    // This file deliberately is not a runnable binary. Static checks must never
    // execute it, and must not claim to certify binary format or task correctness.
    let executable = root.join("private-agent with spaces.exe");
    fs::write(&executable, "not a real binary").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let program = executable.to_str().unwrap().to_owned();
    config.harnesses.insert(
        "agent".into(),
        HarnessConfig {
            auto_verify: true,
            observations: Default::default(),
            command: program.clone(),
            args: vec!["{model}".into(), "{task}".into(), "private-argument".into()],
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "private-model".into()))
                .collect(),
            verification: Some(VerificationConfig {
                command: program,
                args: vec!["private-verifier-argument".into()],
            }),
        },
    );
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    fs::write(root.join(".jevia/runs.jsonl"), "private-broken-history").unwrap();
    fs::write(root.join(".jevia/cache.jsonl"), "private-broken-cache").unwrap();
    config
}

fn report(root: &Path, success: bool) -> Value {
    let result = cli(root)
        .args(["harness", "check", "agent", "--json"])
        .assert();
    let result = if success {
        result.success()
    } else {
        result.failure()
    };
    let output = result.get_output();
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-"));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn harness_preflight_is_offline_read_only_and_structured_on_success_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut config = fixture(root);
    let before = files(root);
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let good = report(&nested, true);
    assert_eq!(good["schema_version"], 1);
    assert_eq!(good["scope"], "static");
    assert_eq!(good["ok"], true);
    assert_eq!(files(root), before);
    assert!(!nested.join(".jevia").exists());
    let text = cli(root)
        .args(["harness", "check", "agent"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&text).contains("static checks only"));
    assert!(!String::from_utf8_lossy(&text).contains("private-"));

    config
        .harnesses
        .get_mut("agent")
        .unwrap()
        .verification
        .as_mut()
        .unwrap()
        .command = root
        .join("private-missing-verifier.exe")
        .to_str()
        .unwrap()
        .into();
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let before = files(root);
    let failed = report(root, false);
    assert!(
        failed["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "verification" && c["code"] == "executable_not_found")
    );
    assert_eq!(files(root), before);
    config.harnesses.get_mut("agent").unwrap().verification = None;
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let warned = report(root, true);
    assert!(
        warned["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "verification_not_configured" && c["status"] == "warning")
    );
    config.harnesses.get_mut("agent").unwrap().command = root
        .join("private-missing-agent.exe")
        .to_str()
        .unwrap()
        .into();
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    assert_eq!(report(root, false)["ok"], false);
}

#[test]
fn harness_preflight_reports_config_errors_without_raw_values_and_accepts_setup_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    assert_eq!(
        report(root, false)["checks"][1]["code"],
        "harness_not_configured"
    );
    let executable = std::env::current_exe().unwrap();
    cli(root)
        .args(["harness", "setup", "agent", "--command"])
        .arg(&executable)
        .args([
            "--arg={model}",
            "--arg={task}",
            "--model=fast=small",
            "--model=balanced=standard",
            "--model=strong=frontier",
            "--apply",
        ])
        .assert()
        .success();
    assert_eq!(report(root, true)["ok"], true);
    let path = root.join(".jevia/config.toml");
    let original = fs::read_to_string(&path).unwrap();
    for (invalid, code) in [
        (
            original.replace("{task}", "{private-unknown}"),
            "invalid_template",
        ),
        (
            original.replace("fast = \"small\"", "fast = \"\""),
            "missing_model_mapping",
        ),
        ("private-malformed = [".into(), "invalid_configuration"),
    ] {
        fs::write(&path, &invalid).unwrap();
        assert_eq!(report(root, false)["checks"][0]["code"], code);
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
}

#[test]
fn harness_preflight_never_launches_agent_or_verifier_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut config = fixture(root);
    let script = root.join(if cfg!(windows) {
        "marker.cmd"
    } else {
        "marker.sh"
    });
    fs::write(
        &script,
        if cfg!(windows) {
            "@echo off\r\necho launched>\"%~1\"\r\n"
        } else {
            "#!/bin/sh\nprintf launched > \"$1\"\n"
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let agent_marker = root.join("agent-launched");
    let verifier_marker = root.join("verifier-launched");
    let harness = config.harnesses.get_mut("agent").unwrap();
    harness.command = script.to_str().unwrap().into();
    harness.args = vec![
        agent_marker.to_str().unwrap().into(),
        "{model}".into(),
        "{task}".into(),
    ];
    harness.verification = Some(VerificationConfig {
        command: script.to_str().unwrap().into(),
        args: vec![verifier_marker.to_str().unwrap().into()],
    });
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    let before = files(root);
    assert_eq!(report(root, true)["ok"], true);
    assert!(!agent_marker.exists());
    assert!(!verifier_marker.exists());
    assert_eq!(files(root), before);
}
