use assert_cmd::Command;
use jevia_core::{Config, StorageConfig};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn cli(root: &Path) -> Command {
    let mut command = Command::cargo_bin("jevia").unwrap();
    command
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env("TOKIO_WORKER_THREADS", "2")
        .timeout(Duration::from_secs(15));
    command
}

fn json_output(root: &Path, args: &[&str]) -> Value {
    let output = cli(root).args(args).assert().success().get_output().clone();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn contract(storage: StorageConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    cli(root).arg("init").assert().success();
    let state = root.join(".jevia");
    let config = Config {
        storage,
        ..Config::default()
    };
    fs::write(state.join("config.toml"), config.to_toml().unwrap()).unwrap();
    let ids: Vec<_> = (0..6).map(|_| uuid::Uuid::new_v4().to_string()).collect();
    let records: Vec<_> = ids.iter().map(|id| json!({
        "schema_version": 6, "run_id": id, "tier": "fast", "suggested_tier": "fast",
        "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
        "jev_model": "fixture", "created_at_ms": 1, "source": "live", "outcome": "unknown",
        "lifecycle": {"state": "completed", "started_at_ms": 1, "finished_at_ms": 2},
        "execution": {"harness": "opencode", "model": "requested", "duration_ms": 1,
            "exit_code": 0, "observations": {"source": "opencode_plugin", "status": "no_events", "events": []}}
    })).collect();
    fs::write(
        state.join("runs.jsonl"),
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    if !config.storage.is_jsonl() {
        cli(root).args(["storage", "init"]).assert().success();
        cli(root)
            .args(["storage", "import-jsonl", "--apply"])
            .assert()
            .success();
    }
    // Protect a valid unreplayed snapshot, a corrupt duplicate journal, a lone
    // loss marker, and even a directory masquerading as a journal. Never open them.
    let journal = state.join(format!("jevia-events-{}-fixture.jsonl", ids[0]));
    let snapshot = json!({"type": "snapshot", "event": {
        "source": "opencode_plugin", "status": "recorded", "events": [
            {"kind": "model_observed", "model": "observed", "recorded_at_ms": 3}
        ]
    }});
    let raw = format!("{snapshot}\n");
    fs::write(&journal, &raw).unwrap();
    fs::write(
        state.join(format!("jevia-events-{}-bad.jsonl", ids[1])),
        "PRIVATE corrupt",
    )
    .unwrap();
    fs::write(
        state.join(format!("jevia-events-{}-duplicate.jsonl", ids[1])),
        "{}",
    )
    .unwrap();
    fs::write(
        state.join(format!("jevia-events-{}-fixture.jsonl.loss", ids[2])),
        "",
    )
    .unwrap();
    fs::create_dir(state.join(format!("jevia-events-{}-unsafe.jsonl", ids[3]))).unwrap();
    let args = ["runs", "archive", "--keep", "1", "--json"];
    let preview = json_output(root, &args);
    assert_eq!(preview["archived_records"], 1);
    assert_eq!(preview["retained_records"], 5);
    assert!(!state.join("history-backups").exists());
    assert_eq!(fs::read_to_string(&journal).unwrap(), raw);
    let applied = json_output(
        root,
        &["runs", "archive", "--keep", "1", "--json", "--apply"],
    );
    assert_eq!(applied["archived_records"], 1);
    let archived: Value =
        serde_json::from_str(&fs::read_to_string(applied["archive"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(archived["run_id"], ids[4]);
    assert_eq!(fs::read_to_string(&journal).unwrap(), raw);
    for id in &ids[..4] {
        json_output(root, &["runs", "show", id, "--json"]);
    }
    // Routing replays before checking credentials. No provider request is made;
    // the expected missing-key error follows a successful local replay.
    cli(root)
        .args(["route", "fixture", "--no-cache"])
        .assert()
        .failure();
    assert!(!journal.exists());
    let saved = json_output(root, &["runs", "show", &ids[0], "--json"]);
    assert_eq!(
        saved["execution"]["observations"]["events"][0]["model"],
        "observed"
    );
    let applied = json_output(
        root,
        &["runs", "archive", "--keep", "1", "--json", "--apply"],
    );
    assert_eq!(applied["archived_records"], 1);
    let archived: Value =
        serde_json::from_str(&fs::read_to_string(applied["archive"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        archived, saved,
        "only archive after recording was durably saved"
    );
}

#[test]
fn jsonl_archive_preserves_pending_recordings_until_replay() {
    contract(StorageConfig::Jsonl);
}

#[test]
fn sqlite_archive_preserves_pending_recordings_until_replay() {
    contract(StorageConfig::Sqlite {
        url: "sqlite://.jevia/archive.db".into(),
    });
}

#[test]
#[ignore = "requires an isolated PostgreSQL database in JEVIA_TEST_POSTGRES_URL"]
fn postgres_archive_preserves_pending_recordings_until_replay() {
    contract(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: format!("archive-recordings-{}", uuid::Uuid::new_v4()),
        allow_insecure_localhost: true,
    });
}
