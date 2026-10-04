use std::fs;

use assert_cmd::Command;
use tempfile::{TempDir, tempdir};

const PRIVATE: &str = "private-value-must-not-appear";

fn project() -> TempDir {
    let dir = tempdir().unwrap();
    Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();
    dir
}

fn fails_privately(dir: &TempDir, args: &[&str]) -> String {
    let output = Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(dir.path())
        .env_remove("TYPESAFE_API_KEY")
        .args(args)
        .assert()
        .failure()
        .get_output()
        .clone();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !text.contains(PRIVATE),
        "diagnostic exposed a private value: {text}"
    );
    text
}

#[test]
fn configuration_errors_redact_parse_values_and_validation_payloads() {
    let dir = project();
    let path = dir.path().join(".jevia/config.toml");
    let original = fs::read_to_string(&path).unwrap();
    let invalid = original.replace(
        "history_limit = 20",
        &format!("history_limit = '{PRIVATE}'"),
    );
    assert_ne!(original, invalid);
    fs::write(&path, &invalid).unwrap();
    for args in [
        vec!["doctor"],
        vec!["route", "test", "--json"],
        vec!["runs", "--json"],
        vec!["storage", "check"],
        vec!["harness", "review", "codex"],
        vec!["harness", "review", "codex", "--launch"],
    ] {
        let diagnostic = fails_privately(&dir, &args);
        assert!(diagnostic.contains("line "));
        assert!(diagnostic.contains("redacted"));
    }
    let invalid = original.replace(
        "fallback_tier = \"strong\"",
        &format!("fallback_tier = '{PRIVATE}'"),
    );
    assert_ne!(original, invalid);
    fs::write(&path, &invalid).unwrap();
    for args in [
        vec!["doctor"],
        vec!["harness", "review", "codex"],
        vec!["harness", "review", "codex", "--launch"],
    ] {
        assert!(fails_privately(&dir, &args).contains("fallback"));
    }
    let invalid = format!(
        "{original}\n[harnesses.test]\ncommand = 'agent'\nargs = ['{{task}}', '{{model}}', '{PRIVATE}-{{unsupported}}']\n[harnesses.test.models]\nfast = 'a'\nbalanced = 'b'\nstrong = 'c'\n"
    );
    fs::write(&path, &invalid).unwrap();
    for args in [
        vec!["doctor"],
        vec!["harness", "review", "codex"],
        vec!["harness", "review", "codex", "--launch"],
    ] {
        assert!(fails_privately(&dir, &args).contains("placeholder"));
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    assert!(!dir.path().join(".jevia/runs.jsonl").exists());
}

fn record() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 3, "run_id": "test", "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
        "jev_model": "test", "created_at_ms": 1, "task": null, "source": "live", "outcome": "unknown"
    })
}

#[test]
fn history_read_and_maintenance_errors_never_echo_record_contents() {
    let dir = project();
    let path = dir.path().join(".jevia/runs.jsonl");
    let mut value = record();
    value["outcome"] = PRIVATE.into();
    let invalid = format!("{value}\n");
    fs::write(&path, &invalid).unwrap();
    for args in [
        vec!["runs", "--json"],
        vec!["stats", "--json"],
        vec!["doctor"],
        vec!["route", "test", "--json"],
        vec!["feedback", "test", "success"],
        vec!["storage", "check"],
        vec!["runs", "repair", "--json"],
        vec!["runs", "archive", "--keep", "1", "--json"],
    ] {
        assert!(fails_privately(&dir, &args).contains("line 1"));
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
}

#[test]
fn cache_errors_and_nonfatal_routing_warnings_redact_values() {
    let dir = project();
    let path = dir.path().join(".jevia/cache.jsonl");
    let mut decision = record();
    decision["source"] = PRIVATE.into();
    let invalid = format!(
        "{}\n",
        serde_json::json!({
            "schema_version": 1, "key": "test", "created_at_ms": 1,
            "expires_at_ms": 9999999999999_u64, "decision": decision,
        })
    );
    fs::write(&path, &invalid).unwrap();
    for args in [
        vec!["cache", "status"],
        vec!["doctor"],
        vec!["route", "test", "--json"],
    ] {
        let diagnostic = fails_privately(&dir, &args);
        assert!(diagnostic.contains("line 1"));
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
}
