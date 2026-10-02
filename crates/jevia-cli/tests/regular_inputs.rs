use assert_cmd::Command;
use std::{fs, path::Path, time::Duration};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2")
        .timeout(Duration::from_secs(5));
    command
}

fn operations(cache: bool) -> Vec<Vec<&'static str>> {
    if cache {
        vec![vec!["cache", "status"]]
    } else {
        vec![
            vec!["runs", "repair", "--json"],
            vec!["runs", "repair", "--apply", "--json"],
            vec!["runs", "archive", "--keep", "1", "--json"],
            vec!["runs", "archive", "--keep", "1", "--apply", "--json"],
        ]
    }
}

fn rejection_contract(create: impl Fn(&Path)) {
    for cache in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        cli(dir.path()).arg("init").assert().success();
        let state = dir.path().join(".jevia");
        let path = state.join(if cache { "cache.jsonl" } else { "runs.jsonl" });
        create(&path);
        let kind = fs::symlink_metadata(&path).unwrap().file_type();
        for args in operations(cache) {
            // Check the normal error exit AND message: a watchdog killing a
            // blocked process must never count as a passing rejection test.
            let output = cli(dir.path())
                .args(args)
                .assert()
                .code(1)
                .get_output()
                .clone();
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("input must be a regular file")
            );
            assert_eq!(fs::symlink_metadata(&path).unwrap().file_type(), kind);
            assert!(!state.join("history-backups").exists());
            assert!(!state.join("history-archives").exists());
        }
    }
}

#[test]
fn directories_are_rejected_without_mutation() {
    rejection_contract(|path| fs::create_dir(path).unwrap());
}

#[test]
#[cfg(unix)]
fn named_pipes_and_links_to_them_never_wait_for_a_writer() {
    use std::{os::unix::fs::symlink, process::Command as Process};
    for link in [false, true] {
        rejection_contract(|path| {
            let target = if link {
                path.with_extension("fifo")
            } else {
                path.to_owned()
            };
            assert!(
                Process::new("mkfifo")
                    .arg(&target)
                    .status()
                    .unwrap()
                    .success()
            );
            if link {
                symlink(target, path).unwrap();
            }
        });
    }
}

#[test]
fn missing_and_empty_regular_files_keep_their_existing_behavior() {
    for cache in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        cli(dir.path()).arg("init").assert().success();
        let state = dir.path().join(".jevia");
        let path = state.join(if cache { "cache.jsonl" } else { "runs.jsonl" });
        for present in [false, true] {
            if present {
                fs::write(&path, "").unwrap();
            }
            for args in operations(cache) {
                cli(dir.path()).args(args).assert().success();
                assert_eq!(path.exists(), present);
                assert!(!state.join("history-backups").exists());
                assert!(!state.join("history-archives").exists());
            }
        }
    }
}

#[test]
#[cfg(unix)]
fn symlinks_to_regular_inputs_remain_readable() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    let state = dir.path().join(".jevia");
    let target = dir.path().join("empty.jsonl");
    fs::write(&target, "").unwrap();
    for name in ["cache.jsonl", "runs.jsonl"] {
        symlink(&target, state.join(name)).unwrap();
    }
    cli(dir.path()).args(["cache", "status"]).assert().success();
    cli(dir.path())
        .args(["runs", "repair", "--json"])
        .assert()
        .success();
    assert_eq!(fs::read(&target).unwrap(), b"");
}
