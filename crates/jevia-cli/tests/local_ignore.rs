use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args(["-c", "core.excludesFile="])
        .args(args)
        .output()
        .expect("Git is required by this integration test")
}

#[test]
fn initialized_projects_ignore_runtime_artifacts_but_not_policy() {
    let root = tempfile::tempdir().unwrap();
    assert!(git(root.path(), &["init", "--quiet"]).status.success());
    assert_cmd::Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(root.path())
        .arg("init")
        .assert()
        .success();
    let private = [
        ".jevia/jevia-events-run-session.jsonl",
        ".jevia/jevia-events-run-session.loss",
        ".jevia/.jevia-event-checkpoint-private",
        ".jevia/jevia-observer-session.mjs",
        ".jevia/event-leases/private.lock",
        ".jevia/.gitignore.lock",
        ".jevia/.jevia-ignore-private.tmp",
        ".jevia/runs.jsonl",
        ".jevia/cache.jsonl",
    ];
    for path in private {
        let result = git(
            root.path(),
            &["check-ignore", "--no-index", "--quiet", "--", path],
        );
        assert!(
            result.status.success(),
            "runtime path was not ignored: {path}"
        );
    }
    for path in [".jevia/config.toml", ".jevia/.gitignore", "src/main.rs"] {
        assert_eq!(
            git(
                root.path(),
                &["check-ignore", "--no-index", "--quiet", "--", path]
            )
            .status
            .code(),
            Some(1),
            "project source/policy was hidden: {path}"
        );
    }
    // Ignore rules never silently remove already-tracked data from the index.
    let history = root.path().join(".jevia/runs.jsonl");
    fs::write(&history, "fixture").unwrap();
    assert!(
        git(root.path(), &["add", "--force", ".jevia/runs.jsonl"])
            .status
            .success()
    );
    assert_cmd::Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(root.path())
        .args(["init", "--force"])
        .assert()
        .success();
    let tracked = git(root.path(), &["ls-files", "--", ".jevia/runs.jsonl"]);
    assert_eq!(
        String::from_utf8(tracked.stdout).unwrap().trim(),
        ".jevia/runs.jsonl"
    );
}
