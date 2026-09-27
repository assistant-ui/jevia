use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("jevia").unwrap();
    cmd.current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("JEVIA_DATABASE_URL")
        .env("TOKIO_WORKER_THREADS", "2");
    cmd
}

fn setup(root: &Path) -> Command {
    let mut cmd = cli(root);
    cmd.args([
        "harness",
        "setup",
        "agent",
        "--command",
        "definitely-missing-agent",
        "--arg=run",
        "--arg=--model",
        "--arg={model}",
        "--arg={task}",
        "--model=fast=provider/small",
        "--model=balanced=provider/standard",
        "--model=strong=provider/frontier",
    ]);
    cmd
}

fn init(root: &Path) -> String {
    cli(root).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Postgres {
            url_env: "JEVIA_DATABASE_URL".into(),
            project: "offline-setup".into(),
            allow_insecure_localhost: false,
        },
        ..Config::default()
    };
    let original = format!("# retain policy\n{}", config.to_toml().unwrap());
    fs::write(root.join(".jevia/config.toml"), &original).unwrap();
    fs::write(root.join(".jevia/runs.jsonl"), "private-invalid-history").unwrap();
    fs::write(root.join(".jevia/cache.jsonl"), "private-invalid-cache").unwrap();
    original
}

#[test]
fn harness_setup_preview_apply_backup_noop_and_explicit_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    let state = root.join(".jevia");
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let preview = setup(&nested)
        .args(["--verify-command", "cargo", "--verify-arg=test"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview = String::from_utf8(preview).unwrap();
    assert!(preview.contains("Preview harness setup"));
    assert!(preview.contains("[harnesses.agent]"));
    assert!(!preview.contains("private-"));
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        original
    );
    assert!(!state.join("config-backups").exists());
    assert!(!state.join("config.lock").exists());
    setup(&nested)
        .args(["--verify-command", "cargo", "--verify-arg=test", "--apply"])
        .assert()
        .success();
    let current = fs::read_to_string(state.join("config.toml")).unwrap();
    let mut config = Config::from_toml(&current).unwrap();
    assert_eq!(
        config.harnesses["agent"].command,
        "definitely-missing-agent"
    );
    let invocation = config.harnesses["agent"]
        .invocation("agent", "fast", "$(touch do-not-run)", "test-id", &[])
        .unwrap();
    assert_eq!(
        invocation.args,
        ["run", "--model", "provider/small", "$(touch do-not-run)"]
    );
    config.harnesses.clear();
    assert_eq!(config, Config::from_toml(&original).unwrap());
    assert!(current.contains("# retain policy"));
    let backups: Vec<_> = fs::read_dir(state.join("config-backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
    // Omitting verifier flags keeps the configured verifier, making this a no-op.
    setup(root).arg("--apply").assert().success();
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        current
    );
    assert_eq!(
        fs::read_dir(state.join("config-backups")).unwrap().count(),
        1
    );
    setup(root)
        .args(["--no-verification", "--apply"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        current
    );
    setup(root)
        .args(["--no-verification", "--apply", "--replace"])
        .assert()
        .success();
    let config =
        Config::from_toml(&fs::read_to_string(state.join("config.toml")).unwrap()).unwrap();
    assert!(config.harnesses["agent"].verification.is_none());
    assert_eq!(
        fs::read_to_string(state.join("runs.jsonl")).unwrap(),
        "private-invalid-history"
    );
    assert_eq!(
        fs::read_to_string(state.join("cache.jsonl")).unwrap(),
        "private-invalid-cache"
    );
    assert!(!nested.join(".jevia").exists());
    assert!(!root.join("do-not-run").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn harness_setup_refuses_backup_failures_invalid_templates_and_verifier_flags() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    let state = root.join(".jevia");
    setup(root).arg("--verify-arg=test").assert().failure();
    setup(root)
        .args(["--verify-command=cargo", "--no-verification"])
        .assert()
        .failure();
    let failure = setup(root)
        .args(["--arg={private-placeholder}", "--apply"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(!String::from_utf8_lossy(&failure).contains("private-placeholder"));
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        original
    );
    fs::write(state.join("config-backups"), "blocked").unwrap();
    setup(root).arg("--apply").assert().failure();
    assert_eq!(
        fs::read_to_string(state.join("config.toml")).unwrap(),
        original
    );
}

#[test]
#[cfg(unix)]
fn harness_setup_refuses_symlink_configuration() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = init(root);
    let state = root.join(".jevia");
    fs::rename(state.join("config.toml"), state.join("original.toml")).unwrap();
    symlink(state.join("original.toml"), state.join("config.toml")).unwrap();
    setup(root).arg("--apply").assert().failure();
    assert_eq!(
        fs::read_to_string(state.join("original.toml")).unwrap(),
        original
    );
}
