use std::{fs, path::Path};

use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::{Value, json};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2");
    command
}

fn stats(root: &Path, args: &[&str]) -> Value {
    let bytes = cli(root)
        .arg("stats")
        .arg("--json")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&bytes).unwrap()
}

fn fixture() -> Vec<Value> {
    let mut records = Vec::new();
    for (index, (tier, outcome, source)) in [
        ("fast", "success", Some("verification")),
        ("fast", "failure", Some("verification")),
        ("strong", "failure", Some("manual")),
        ("strong", "success", Some("process_exit")),
        ("retired-tier", "success", None),
        ("fast", "unknown", None),
    ]
    .into_iter()
    .enumerate()
    {
        let mut record = json!({
            "schema_version": 3, "run_id": format!("private-id-{index}"),
            "tier": tier, "suggested_tier": "balanced", "confidence": 0.9,
            "probabilities": {}, "fallback_applied": true, "jev_model": "private-model",
            "created_at_ms": 100 - index, "task": "private-task", "outcome": outcome,
            "source": if index == 0 { "cache" } else { "live" },
            "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2}
        });
        if let Some(source) = source {
            record["outcome_evidence"] = json!({"source": source, "recorded_at_ms": 2});
        }
        if index < 4 {
            record["execution"] = json!({"harness": "private-harness", "model": "private-execution-model", "duration_ms": 10, "exit_code": 0});
        }
        if index < 3 {
            record["execution"]["verification"] = json!({"command": "private-verifier", "launched": true, "duration_ms": 5, "exit_code": if index == 1 { 1 } else { 0 }});
        }
        if index == 0 {
            record["schema_version"] = json!(5);
            record["execution"]["observations"] = json!({
                "source": "claude_hooks", "status": "partial",
                "events": [
                    {"kind":"tool_failed", "recorded_at_ms":1, "session_id":"private-session", "model":"actual-a"},
                    {"kind":"model_changed", "recorded_at_ms":2, "previous_model":"actual-a", "model":"actual-b"}
                ],
                "totals": {
                    "event_counts": {"tool_failed":300,"model_changed":1},
                    "models": {"actual-a":{"tool_failed":290},"actual-b":{"model_changed":1}},
                    "unattributed_event_counts":{"tool_failed":10}, "omitted_model_event_counts":{},
                    "models_truncated":false, "discarded_inputs":1
                }
            });
        }
        if index == 2 {
            record["feedback"] = json!([
                {"previous_outcome": "success", "previous_source": "verification", "outcome": "failure", "recorded_at_ms": 3, "reason": "private-reason"},
                {"previous_outcome": "failure", "previous_source": "manual", "outcome": "failure", "recorded_at_ms": 4, "reason": "private-reason"}
            ]);
        }
        if index == 4 {
            record["schema_version"] = json!(1);
            record.as_object_mut().unwrap().remove("lifecycle");
            record.as_object_mut().unwrap().remove("source");
        }
        records.push(record);
    }
    records
}

fn history(records: &[Value]) -> String {
    records.iter().map(|record| format!("{record}\n")).collect()
}

fn exercise(backend: StorageConfig, name: &str) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    cli(root).arg("init").assert().success();
    let mut config = Config {
        storage: backend,
        ..Config::default()
    };
    // Any accidental Jev request would fail: this command requires neither a
    // credential nor an API connection (PostgreSQL still needs its database).
    config.jev.base_url = "http://127.0.0.1:1".into();
    fs::write(root.join(".jevia/config.toml"), config.to_toml().unwrap()).unwrap();
    if name != "jsonl" {
        cli(root).args(["storage", "init"]).assert().success();
    }
    let empty = stats(root, &[]);
    assert_eq!(empty["totals"]["records"], 0);
    assert!(empty["totals"]["verified_success_rate"].is_null());
    assert!(empty["totals"]["cache_hit_rate"].is_null());
    assert_eq!(empty["window"]["has_older_records"], false);
    let original = history(&fixture());
    fs::write(root.join(".jevia/runs.jsonl"), &original).unwrap();
    // Stats concerns recorded decisions, not current cache entries or corruption.
    fs::write(root.join(".jevia/cache.jsonl"), "private-cache-not-json").unwrap();
    if name != "jsonl" {
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
    }
    let report = stats(root, &[]);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["storage"], name);
    assert_eq!(
        report["window"],
        json!({"limit": 1000, "order": "append", "has_older_records": false})
    );
    assert_eq!(
        report["totals"],
        json!({
            "records": 6, "cache_hits": 1, "learning_evidence": 3,
            "verified": {"successes": 1, "failures": 1},
            "manual": {"successes": 0, "failures": 1},
            "process_exit": {"successes": 1, "failures": 0},
            "unattributed": {"successes": 1, "failures": 0},
            "active": 0, "unknown": 1, "verified_success_rate": 0.5, "cache_hit_rate": 1.0 / 6.0
        })
    );
    assert_eq!(report["tiers"]["fast"]["records"], 3);
    assert_eq!(report["tiers"]["fast"]["verified_success_rate"], 0.5);
    assert_eq!(report["tiers"]["strong"]["manual"]["failures"], 1);
    assert!(report["tiers"]["strong"]["verified_success_rate"].is_null());
    assert!(report["tiers"].get("retired-tier").is_some());
    assert!(report["tiers"].get("balanced").is_none());
    assert_eq!(report["observations"]["coverage"]["partial"], 1);
    assert_eq!(report["observations"]["coverage"]["not_reported"], 3);
    assert_eq!(report["observations"]["sampled_runs"], 1);
    assert_eq!(report["observations"]["event_counts"]["tool_failed"], 300);
    assert_eq!(
        report["observations"]["models"]["actual-a"]["event_counts"]["tool_failed"],
        290
    );
    assert_eq!(report["observations"]["models"]["actual-b"]["runs"], 1);
    assert_eq!(
        report["observations"]["unattributed_event_counts"]["tool_failed"],
        10
    );
    let subdir = root.join("nested");
    fs::create_dir(&subdir).unwrap();
    assert_eq!(stats(&subdir, &[]), report);
    let limited = stats(root, &["--limit", "2"]);
    assert_eq!(limited["window"]["has_older_records"], true);
    assert_eq!(limited["totals"]["records"], 2);
    assert_eq!(limited["totals"]["unattributed"]["successes"], 1);
    assert_eq!(limited["totals"]["unknown"], 1);
    assert!(limited["totals"]["verified_success_rate"].is_null());
    assert_eq!(
        stats(root, &["--limit", "6"])["window"]["has_older_records"],
        false
    );
    let output = cli(root)
        .arg("stats")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Verified success: 1/2 (50.0%)"));
    assert!(output.contains("Manual ok/fail"));
    assert!(output.contains("Observed model switches: 1"));
    assert!(output.contains("actual-a"));
    assert!(output.contains("n/a"));
    assert!(!output.contains("private-"));
    assert!(!report.to_string().contains("private-"));
    assert_eq!(
        fs::read_to_string(root.join(".jevia/runs.jsonl")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(root.join(".jevia/cache.jsonl")).unwrap(),
        "private-cache-not-json"
    );
    assert_eq!(stats(root, &[]), report); // No probes, fake evidence or new runs.
    report
}

#[test]
fn stats_jsonl_and_sqlite_have_identical_aggregates() {
    let jsonl = exercise(StorageConfig::Jsonl, "jsonl");
    let sqlite = exercise(
        StorageConfig::Sqlite {
            url: "sqlite://.jevia/stats.db".into(),
        },
        "sqlite",
    );
    assert_eq!(jsonl["totals"], sqlite["totals"]);
    assert_eq!(jsonl["tiers"], sqlite["tiers"]);
    assert_eq!(jsonl["observations"], sqlite["observations"]);
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_stats_matches_jsonl_and_is_project_scoped() {
    let jsonl = exercise(StorageConfig::Jsonl, "jsonl");
    // Reusing the same IDs in two namespaces also checks project-scoped reads.
    for _ in 0..2 {
        let postgres = exercise(
            StorageConfig::Postgres {
                url_env: "JEVIA_TEST_POSTGRES_URL".into(),
                project: format!("stats-{}", uuid::Uuid::new_v4()),
                allow_insecure_localhost: true,
            },
            "postgres",
        );
        assert_eq!(jsonl["totals"], postgres["totals"]);
        assert_eq!(jsonl["tiers"], postgres["tiers"]);
        assert_eq!(jsonl["observations"], postgres["observations"]);
    }
}

#[test]
fn stats_rejects_invalid_limits_and_does_not_initialize_missing_database() {
    let directory = tempfile::tempdir().unwrap();
    for limit in ["0", "100001", "-1", "invalid"] {
        cli(directory.path())
            .args(["stats", "--limit", limit])
            .assert()
            .failure();
    }
    assert!(!directory.path().join(".jevia").exists());
    cli(directory.path()).arg("init").assert().success();
    let config = Config {
        storage: StorageConfig::Sqlite {
            url: "sqlite://.jevia/missing.db".into(),
        },
        ..Config::default()
    };
    fs::write(
        directory.path().join(".jevia/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    cli(directory.path()).arg("stats").assert().failure();
    assert!(!directory.path().join(".jevia/missing.db").exists());
    assert!(!directory.path().join(".jevia/runs.jsonl").exists());
}

#[test]
fn stats_refuses_corrupt_and_unsupported_history_even_outside_window() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    cli(root).arg("init").assert().success();
    let mut unsupported = fixture()[0].clone();
    unsupported["schema_version"] = json!(999);
    for prefix in ["{invalid\n".to_owned(), format!("{unsupported}\n")] {
        let contents = prefix + &history(&fixture());
        fs::write(root.join(".jevia/runs.jsonl"), &contents).unwrap();
        let output = cli(root)
            .args(["stats", "--limit", "1", "--json"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(".jevia/runs.jsonl")).unwrap(),
            contents
        );
    }
}
