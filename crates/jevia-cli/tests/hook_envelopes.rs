use std::{fs, time::Duration};

use assert_cmd::Command;
use serde_json::{Value, json};

#[test]
fn large_tool_bodies_preserve_native_metadata_without_persisting_content() {
    let root = tempfile::tempdir().unwrap();
    for (source, expected) in [
        ("claude_hooks", "tool_succeeded"),
        ("codex_hooks", "tool_completed"),
        ("opencode_plugin", "tool_completed"),
    ] {
        let journal = root.path().join(format!("jevia-events-{source}.jsonl"));
        fs::write(
            &journal,
            json!({"type":"snapshot", "event":{
                "source":source, "status":"no_events", "events":[]
            }})
            .to_string(),
        )
        .unwrap();
        let payload = json!({"hook_event_name":"PostToolUse", "session_id":"session-1",
            "model":"actual-model", "tool_name":"Read", "tool_response":"PRIVATE".repeat(12_000)})
        .to_string();
        assert!(payload.len() > 64 * 1024);
        let receive = |payload: String| {
            Command::cargo_bin("jevia")
                .unwrap()
                .current_dir(root.path())
                .args(["capture-event", "--source", source, "--journal"])
                .arg(&journal)
                .timeout(Duration::from_secs(10))
                .write_stdin(payload)
                .assert()
                .success()
                .stdout("");
        };
        receive(payload);
        let raw = fs::read_to_string(&journal).unwrap();
        assert!(raw.len() < 1024);
        assert!(!raw.contains("PRIVATE"));
        let snapshot: Value = serde_json::from_str(&raw).unwrap();
        let observed = &snapshot["event"];
        assert_eq!(observed["source"], source);
        assert_eq!(observed["status"], "recorded");
        assert_eq!(observed["events"].as_array().unwrap().len(), 1);
        assert_eq!(observed["events"][0]["kind"], expected);
        assert_eq!(observed["totals"]["models"]["actual-model"][expected], 1);
        assert_eq!(observed["totals"]["discarded_inputs"], 0);
        let mut too_large = r#"{"hook_event_name":"Stop"}"#.to_owned();
        too_large.extend(std::iter::repeat_n(' ', 8 * 1024 * 1024));
        receive(too_large);
        receive(r#"{"hook_event_name":"Stop","private": [}"#.into());
        let snapshot: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        assert_eq!(snapshot["event"]["status"], "partial");
        assert_eq!(snapshot["event"]["events"], observed["events"]);
        assert_eq!(snapshot["event"]["totals"]["discarded_inputs"], 2);
    }
}
