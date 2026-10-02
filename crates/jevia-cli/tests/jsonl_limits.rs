use assert_cmd::Command;
use std::fs;

#[test]
fn oversized_history_is_rejected_without_mutation_or_database_creation() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("jevia")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();
    let path = dir.path().join(".jevia/runs.jsonl");
    let raw = format!(
        "{{\"private_padding\":\"{}\"}}\n",
        "x".repeat(8 * 1024 * 1024)
    );
    fs::write(&path, &raw).unwrap();
    for args in [
        vec!["storage", "check"],
        vec!["storage", "check", "--deep"],
        vec!["history", "--json"],
        vec!["storage", "setup", "sqlite", "--import-jsonl"],
    ] {
        let output = Command::cargo_bin("jevia")
            .unwrap()
            .current_dir(dir.path())
            .args(args)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stderr.len() < 4096);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private_padding"));
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        assert!(!dir.path().join(".jevia/jevia.db").exists());
    }
}
