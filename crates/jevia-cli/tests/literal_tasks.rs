#![cfg(unix)]

use assert_cmd::Command;
use jevia_core::{Config, HarnessConfig, ObservationMode, RouteDecision};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

#[test]
fn leading_dash_task_reaches_the_harness_and_record_without_becoming_an_option() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join(".jevia");
    fs::create_dir(&state).unwrap();
    let executable = dir.path().join("claude");
    fs::write(&executable, "#!/bin/sh\nprintf '%s\\0' \"$@\" > argv.bin\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = Config::default();
    config.harnesses.insert(
        "agent".into(),
        HarnessConfig {
            command: executable.to_str().unwrap().into(),
            args: ["--print", "--model", "{model}", "{task}"]
                .map(String::from)
                .into(),
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "requested".into()))
                .collect(),
            observations: ObservationMode::Off,
            auto_verify: false,
            verification: None,
        },
    );
    fs::write(state.join("config.toml"), config.to_toml().unwrap()).unwrap();
    for task in ["--help", "- Fix the parser", "- first\n- second"] {
        let key = jevia_core::route_cache_key(task, Some("agent"), &config, &[]).unwrap();
        let decision: RouteDecision = serde_json::from_value(json!({
            "run_id":"cached", "tier":"fast", "suggested_tier":"fast", "confidence":0.99,
            "probabilities":{}, "fallback_applied":false, "jev_model":"fixture", "created_at_ms":1
        }))
        .unwrap();
        // No provider call: a correctly scoped cache hit supplies the decision.
        fs::write(
            state.join("cache.jsonl"),
            format!(
                "{}\n",
                json!({
                    "schema_version":1, "key":key, "decision":decision,
                    "created_at_ms":1, "expires_at_ms":u64::MAX
                })
            ),
        )
        .unwrap();
        // Give each case the same empty-history cache context.
        fs::write(state.join("runs.jsonl"), "").unwrap();
        Command::cargo_bin("jevia")
            .unwrap()
            .current_dir(dir.path())
            .env_remove("TYPESAFE_API_KEY")
            .env("TOKIO_WORKER_THREADS", "2")
            .args([
                "run",
                "agent",
                &format!("--task={task}"),
                "--non-interactive",
                "--",
                "--verbose",
            ])
            .timeout(Duration::from_secs(10))
            .assert()
            .success();
        let argv = fs::read(dir.path().join("argv.bin")).unwrap();
        let expected =
            ["--print", "--model", "requested", "--verbose", "--", task].join("\0") + "\0";
        assert_eq!(argv, expected.as_bytes());
        let record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.join("runs.jsonl")).unwrap()).unwrap();
        assert_eq!(record["task"], task);
        assert_eq!(record["lifecycle"]["state"], "completed");
        assert_eq!(record["outcome"], "unknown");
    }
}
