use assert_cmd::Command;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn record(id: &str) -> Value {
    json!({
        "schema_version": 6, "run_id": id, "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
        "jev_model": "test", "created_at_ms": 1, "source": "live", "outcome": "unknown",
        "task": "PRIVATE task", "additional_metadata": {"PRIVATE": "preserve"},
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2}
    })
}

#[test]
fn maintenance_refuses_invalid_decisions_before_any_rewrite_or_snapshot() {
    for (field, value) in [
        ("confidence", json!(2)),
        ("confidence", json!(-0.1)),
        ("probabilities", json!({"PRIVATE-tier": 1.1})),
        ("probabilities", json!({"PRIVATE-tier": -0.1})),
        ("run_id", json!(" ")),
        ("tier", json!(" ")),
        ("suggested_tier", json!(" ")),
        ("jev_model", json!(" ")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        cli(dir.path()).arg("init").assert().success();
        let state = dir.path().join(".jevia");
        let path = state.join("runs.jsonl");
        let mut invalid = record("invalid");
        invalid[field] = value;
        for original in [
            format!("{invalid}"), // Complete invalid tail, not a truncated JSON value.
            format!("{invalid}\n{}\n", record("latest")), // Invalid archival candidate.
            format!("{}\n{invalid}\n{{", record("first")), // Repairable tail cannot excuse an earlier invalid decision.
        ] {
            fs::write(&path, &original).unwrap();
            cli(dir.path()).args(["storage", "check"]).assert().code(1);
            for apply in [false, true] {
                for mut args in [
                    vec!["runs", "repair"],
                    vec!["runs", "archive", "--keep", "1"],
                ] {
                    if apply {
                        args.push("--apply");
                    }
                    let result = cli(dir.path())
                        .args(args)
                        .assert()
                        .code(1)
                        .get_output()
                        .clone();
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    assert!(stderr.contains("invalid run record on line"), "{stderr}");
                    assert!(stderr.contains("refusing to rewrite history"), "{stderr}");
                    assert!(!stderr.contains("PRIVATE"));
                    assert!(result.stdout.is_empty());
                    assert_eq!(fs::read_to_string(&path).unwrap(), original);
                    assert!(!state.join("history-backups").exists());
                    assert!(!state.join("history-archives").exists());
                }
            }
        }
    }
}

#[test]
fn valid_legacy_and_boundary_decisions_still_repair_and_archive_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    let path = dir.path().join(".jevia/runs.jsonl");
    let mut first = record("first");
    first["schema_version"] = json!(1);
    first["confidence"] = json!(0);
    let mut last = record("last");
    last["confidence"] = json!(1);
    last["probabilities"] = json!({"fast": 1, "strong": 0});
    let original = format!("{first}\n{last}");
    fs::write(&path, &original).unwrap();
    cli(dir.path())
        .args(["runs", "repair", "--apply"])
        .assert()
        .success();
    cli(dir.path())
        .args(["storage", "check"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&path).unwrap(), original.clone() + "\n");
    let output = cli(dir.path())
        .args(["runs", "archive", "--keep", "1", "--apply", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        fs::read_to_string(report["archive"].as_str().unwrap()).unwrap(),
        format!("{first}\n")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{last}\n"));
    cli(dir.path())
        .args(["storage", "check"])
        .assert()
        .success();
}

#[test]
fn maintenance_enforces_the_same_physical_line_bound_as_readers() {
    const LIMIT: usize = 8 * 1024 * 1024;
    let dir = tempfile::tempdir().unwrap();
    cli(dir.path()).arg("init").assert().success();
    let path = dir.path().join(".jevia/runs.jsonl");
    let mut boundary = record("boundary").to_string();
    boundary.extend(std::iter::repeat_n(' ', LIMIT - boundary.len()));
    for raw in [
        boundary.clone(),
        boundary + "\n",
        " ".repeat(LIMIT),
        "{\"PRIVATE\":\"".to_string() + &"x".repeat(LIMIT),
    ] {
        fs::write(&path, &raw).unwrap();
        for args in [
            vec!["runs", "repair", "--apply"],
            vec!["runs", "archive", "--keep", "1", "--apply"],
        ] {
            let output = cli(dir.path())
                .args(args)
                .assert()
                .code(1)
                .get_output()
                .clone();
            assert!(String::from_utf8_lossy(&output.stderr).contains("8 MiB limit"));
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            assert!(!dir.path().join(".jevia/history-backups").exists());
        }
    }
    let mut valid = record("boundary").to_string();
    valid.extend(std::iter::repeat_n(' ', LIMIT - 1 - valid.len()));
    fs::write(&path, &valid).unwrap();
    cli(dir.path())
        .args(["runs", "repair", "--apply"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&path).unwrap(), valid + "\n");
    cli(dir.path())
        .args(["storage", "check"])
        .assert()
        .success();
}
