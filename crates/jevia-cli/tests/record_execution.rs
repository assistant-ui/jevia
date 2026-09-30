use assert_cmd::Command;
use serde_json::json;

#[test]
fn recording_input_rejects_unbounded_or_raw_payloads_without_echoing_them() {
    let root = tempfile::tempdir().unwrap();
    Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(root.path())
        .arg("init")
        .assert()
        .success();
    let valid = json!({"harness":"app","model":"requested","duration_ms":1,"events":[]});
    let mut extra = valid.clone();
    extra["prompt"] = json!("PRIVATE payload");
    let mut event = valid.clone();
    event["events"] =
        json!([{"kind":"turn_completed","recorded_at_ms":1,"output":"PRIVATE payload"}]);
    for input in [
        b"PRIVATE malformed".to_vec(),
        serde_json::to_vec(&extra).unwrap(),
        serde_json::to_vec(&event).unwrap(),
        vec![b'x'; 256 * 1024 + 1],
    ] {
        let result = Command::cargo_bin("jevia")
            .unwrap()
            .current_dir(root.path())
            .env("TOKIO_WORKER_THREADS", "2")
            .args(["runs", "record-execution", "--json", "--", "unknown-run"])
            .write_stdin(input)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE"));
    }
}
