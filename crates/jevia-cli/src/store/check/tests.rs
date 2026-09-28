use super::*;
use serde_json::json;
use std::fs;

fn record(index: usize) -> serde_json::Value {
    json!({
        "schema_version": 1 + index % 3, "run_id": format!("private-id-{index}"), "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false, "jev_model": "test",
        "created_at_ms": 1, "task": "private-task", "outcome": "unknown",
        "additional_metadata": {"private-key": "private-value"}
    })
}

#[test]
fn deep_check_jsonl_streams_valid_history_without_rewriting_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.jsonl");
    assert_eq!(check_deep(&path).unwrap(), 0);
    assert!(!path.exists());
    let original: String = (0..405)
        .map(|index| format!("{}\n \n", record(index)))
        .collect();
    let original = original.trim_end();
    fs::write(&path, original).unwrap();
    assert_eq!(check_deep(&path).unwrap(), 405);
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn deep_check_jsonl_rejects_ambiguous_records_and_redacts_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.jsonl");
    let mut unsupported = record(1);
    unsupported["schema_version"] = json!(999);
    let mut empty = record(1);
    empty["run_id"] = json!("");
    let mut invalid = record(1);
    invalid["outcome"] = json!("private-enum-value");
    let mut bad_probability = record(1);
    bad_probability["probabilities"] = json!({"private-tier": 2});
    let mut blank_model = record(1);
    blank_model["jev_model"] = json!(" \t");
    for bad in [
        "{private-truncated".into(),
        unsupported.to_string(),
        empty.to_string(),
        invalid.to_string(),
        bad_probability.to_string(),
        blank_model.to_string(),
        record(0).to_string(),
    ] {
        let contents = format!("{}\n{bad}\n", record(0));
        fs::write(&path, &contents).unwrap();
        let error = check_deep(&path).unwrap_err();
        assert!(format!("{error:#}").contains("line 2"));
        assert!(!format!("{error:#}").contains("private-"));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(check_deep(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
}
