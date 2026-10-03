use super::*;
use jevia_core::{
    Config, ExecutionEvidence, HarnessObservations, ObservationStatus, Outcome, RouteRecord,
    RunLifecycle, StorageConfig,
};

#[test]
fn retention_inventory_rechecks_new_owners_and_redacts_scan_errors() {
    let dir = tempfile::tempdir().unwrap();
    let first = uuid::Uuid::new_v4().to_string();
    let path = dir
        .path()
        .join(format!("jevia-events-{first}-fixture.jsonl"));
    fs::write(&path, "corrupt payload must not be read").unwrap();
    let pending = PendingRecordings::scan(dir.path()).unwrap();
    assert!(pending.contains(&first));
    pending.recheck(dir.path()).unwrap();
    // A successful replay is safe; newly discovered owners invalidate the plan.
    fs::remove_file(&path).unwrap();
    pending.recheck(dir.path()).unwrap();
    let second = uuid::Uuid::new_v4().to_string();
    fs::write(
        dir.path()
            .join(format!("jevia-events-{second}-fixture.jsonl.loss")),
        "",
    )
    .unwrap();
    assert!(pending.recheck(dir.path()).is_err());
    let private = dir.path().join("PRIVATE-path");
    fs::write(&private, "not a directory").unwrap();
    let error = PendingRecordings::scan(&private).err().unwrap().to_string();
    assert!(error.contains("archival refused"));
    assert!(!error.contains("PRIVATE"));
    let missing = dir.path().join("not-created-yet");
    let pending = PendingRecordings::scan(&missing).unwrap();
    pending.recheck(&missing).unwrap();
    fs::create_dir(&missing).unwrap();
    pending.recheck(&missing).unwrap();
    fs::write(
        missing.join(format!("jevia-events-{second}-fixture.jsonl")),
        "",
    )
    .unwrap();
    assert!(pending.recheck(&missing).is_err());
}

#[cfg(unix)]
#[test]
fn retention_inventory_does_not_open_pipes_or_follow_symlinks() {
    use nix::{sys::stat::Mode, unistd::mkfifo};
    let dir = tempfile::tempdir().unwrap();
    let dangling = dir.path().join("dangling-directory");
    std::os::unix::fs::symlink("missing-target", &dangling).unwrap();
    assert!(PendingRecordings::scan(&dangling).is_err());
    for pipe in [true, false] {
        let id = uuid::Uuid::new_v4().to_string();
        let path = dir.path().join(format!("jevia-events-{id}-fixture.jsonl"));
        if pipe {
            mkfifo(&path, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
        } else {
            std::os::unix::fs::symlink("missing-target", &path).unwrap();
        }
        assert!(PendingRecordings::scan(dir.path()).unwrap().contains(&id));
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    paths: ProjectPaths,
    storage: Storage,
    record: RouteRecord,
}

impl Fixture {
    async fn new(backend: StorageConfig) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().into());
        fs::create_dir_all(&paths.directory).unwrap();
        let storage = Storage::open(
            &Config {
                storage: backend,
                ..Config::default()
            },
            &paths,
            true,
        )
        .await
        .unwrap();
        let sample = crate::tests::sample_record();
        let mut record = RouteRecord::new(sample.decision, sample.task);
        record.decision.run_id = uuid::Uuid::new_v4().to_string();
        record.lifecycle = Some(RunLifecycle {
            state: RunState::Completed,
            started_at_ms: Some(1),
            finished_at_ms: Some(2),
        });
        record.execution = Some(ExecutionEvidence {
            harness: "opencode".into(),
            model: "requested".into(),
            duration_ms: 1,
            exit_code: Some(0),
            verification: None,
            observations: Some(HarnessObservations {
                source: Some(ObservationSource::OpencodePlugin),
                status: ObservationStatus::NoEvents,
                events: vec![],
                totals: None,
            }),
        });
        Self {
            _directory: directory,
            paths,
            storage,
            record,
        }
    }

    fn file(&self, prefix: &str, suffix: &str, bytes: &[u8]) -> PathBuf {
        let path = self.paths.directory.join(format!(
            "{prefix}{}-fixture{suffix}",
            self.record.decision.run_id
        ));
        fs::write(&path, bytes).unwrap();
        path
    }

    fn plugin(&self) -> PathBuf {
        self.file(
            "jevia-observer-",
            ".mjs",
            include_bytes!("../observations/opencode.mjs"),
        )
    }

    fn checkpoint(&self) -> PathBuf {
        let bytes = serde_json::to_vec(&serde_json::json!({"type":"snapshot", "event":self.record.execution.as_ref().unwrap().observations})).unwrap();
        self.file(".jevia-event-checkpoint-", "", &bytes)
    }
}

fn local_backends() -> [StorageConfig; 2] {
    [
        StorageConfig::Jsonl,
        StorageConfig::Sqlite {
            url: "sqlite://.jevia/runs.db".into(),
        },
    ]
}

#[tokio::test]
async fn ineligible_artifacts_skip_rescans_but_every_possible_move_is_rechecked() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    for i in 0..2000 {
        f.file(
            "jevia-observer-",
            &format!("-{i}.mjs"),
            b"not a known plugin",
        );
    }
    let retained = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(retained.moved, 0);
    assert_eq!(retained.journal_scans, 0);
    f.plugin();
    f.checkpoint();
    let moved = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(moved.moved, 2);
    assert_eq!(moved.journal_scans, 2);
    assert!(!has_replay_source(&f.paths.directory, &f.record.decision.run_id).unwrap());
    for extension in [".jsonl", ".loss"] {
        let late = f.file("jevia-events-", extension, b"pending");
        assert!(has_replay_source(&f.paths.directory, &f.record.decision.run_id).unwrap());
        fs::remove_file(late).unwrap();
    }
}

async fn contract(backend: StorageConfig) {
    let f = Fixture::new(backend).await;
    f.storage.append(&f.record).await.unwrap();
    let plugin = f.plugin();
    let checkpoint = f.checkpoint();
    let before = f.storage.get(&f.record.decision.run_id).await.unwrap();
    let preview = cleanup(&f.paths, &f.storage, false).await.unwrap();
    assert_eq!(preview.eligible, 2);
    assert_eq!(preview.moved, 0);
    assert!(!preview.applied);
    assert!(preview.archive.is_none());
    assert!(!f.paths.directory.join("recording-archives").exists());
    assert!(plugin.exists() && checkpoint.exists());
    let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(applied.moved, 2);
    assert!(applied.applied);
    let archive = applied.archive.unwrap();
    assert_eq!(
        fs::read(archive.join(plugin.file_name().unwrap())).unwrap(),
        include_bytes!("../observations/opencode.mjs")
    );
    assert!(archive.join(checkpoint.file_name().unwrap()).exists());
    assert!(!plugin.exists() && !checkpoint.exists());
    assert_eq!(
        f.storage.get(&f.record.decision.run_id).await.unwrap(),
        before
    );
    assert_eq!(before.outcome, Outcome::Unknown);
    assert!(
        fs::read_to_string(f.paths.directory.join(".gitignore"))
            .unwrap()
            .contains("recording-archives/")
    );
    assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
}

#[tokio::test]
async fn cleanup_previews_then_archives_without_changing_history() {
    for backend in local_backends() {
        contract(backend).await;
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_recording_cleanup_contract() {
    contract(StorageConfig::Postgres {
        url_env: "JEVIA_TEST_POSTGRES_URL".into(),
        project: uuid::Uuid::new_v4().to_string(),
        allow_insecure_localhost: true,
    })
    .await;
}

#[tokio::test]
async fn active_pending_unknown_and_missing_owners_are_retained() {
    for backend in local_backends() {
        for state in [
            Some(RunState::Running),
            Some(RunState::Verifying),
            Some(RunState::Routed),
            None,
        ] {
            let mut f = Fixture::new(backend.clone()).await;
            f.record.lifecycle = state.map(|state| RunLifecycle {
                state,
                started_at_ms: Some(1),
                finished_at_ms: None,
            });
            f.storage.append(&f.record).await.unwrap();
            let plugin = f.plugin();
            assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
            assert!(plugin.exists());
        }
        let f = Fixture::new(backend).await;
        let plugin = f.plugin();
        assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
        assert!(plugin.exists());
    }
}

#[tokio::test]
async fn apply_rechecks_execution_lease_journals_and_loss_markers() {
    for backend in local_backends() {
        let f = Fixture::new(backend).await;
        f.storage.append(&f.record).await.unwrap();
        let plugin = f.plugin();
        assert_eq!(
            cleanup(&f.paths, &f.storage, false).await.unwrap().eligible,
            1
        );
        let lease = f
            .storage
            .execution_guard(&f.record.decision.run_id)
            .await
            .unwrap();
        let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
        assert_eq!(applied.moved, 0);
        assert_eq!(applied.retained["execution_busy"], 1);
        assert!(plugin.exists());
        drop(lease);
        for extension in [".jsonl", ".loss"] {
            let journal = f.file("jevia-events-", extension, b"PRIVATE unpersisted input");
            let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
            assert_eq!(applied.moved, 0);
            assert_eq!(applied.retained["replay_source"], 1);
            assert_eq!(applied.retained["pending_journal_or_marker"], 1);
            assert_eq!(fs::read(&journal).unwrap(), b"PRIVATE unpersisted input");
            assert!(plugin.exists());
            fs::remove_file(journal).unwrap();
        }
    }
}

#[tokio::test]
async fn unproven_contents_and_legacy_names_are_never_moved() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    let checkpoint = f.checkpoint();
    let saved = fs::read(&checkpoint).unwrap();
    // A valid but newer snapshot is not cleanup-safe.
    let mut newer: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    newer["event"]["status"] = "partial".into();
    fs::write(&checkpoint, serde_json::to_vec(&newer).unwrap()).unwrap();
    let plugin = f.file("jevia-observer-", ".mjs", b"PRIVATE custom plugin");
    let legacy = f.paths.directory.join("jevia-observer-legacy.mjs");
    fs::write(&legacy, include_bytes!("../observations/opencode.mjs")).unwrap();
    let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(applied.moved, 0);
    assert_eq!(applied.retained["unknown_ownership"], 1);
    assert_eq!(applied.retained["contents_not_known_saved"], 2);
    for path in [&checkpoint, &plugin, &legacy] {
        assert!(path.exists());
    }
    let encoded = serde_json::to_string(&applied).unwrap();
    assert!(!encoded.contains("PRIVATE") && !encoded.contains(&f.record.decision.run_id));
    let mut unknown: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    unknown["event"]["future_field"] = "unpersisted".into();
    fs::write(&checkpoint, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
    assert!(checkpoint.exists());
    fs::write(&checkpoint, vec![b'x'; FILE_LIMIT as usize + 1]).unwrap();
    assert!(read_regular(&checkpoint).is_err());
}

#[tokio::test]
async fn archive_failure_preserves_source_and_history() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    let plugin = f.plugin();
    fs::write(
        f.paths.directory.join("recording-archives"),
        "not a directory",
    )
    .unwrap();
    let history = fs::read(&f.paths.runs).unwrap();
    assert!(cleanup(&f.paths, &f.storage, true).await.is_err());
    assert_eq!(
        fs::read(plugin).unwrap(),
        include_bytes!("../observations/opencode.mjs")
    );
    assert_eq!(fs::read(&f.paths.runs).unwrap(), history);
}

#[cfg(unix)]
#[tokio::test]
async fn linked_files_and_archive_directory_are_rejected() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    let plugin = f.plugin();
    let other = f.paths.directory.join("other");
    fs::hard_link(&plugin, &other).unwrap();
    assert!(read_regular(&plugin).is_err());
    fs::remove_file(&plugin).unwrap();
    std::os::unix::fs::symlink(&other, &plugin).unwrap();
    assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
    assert!(fs::symlink_metadata(&plugin).unwrap().is_symlink());
    let destination = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        destination.path(),
        f.paths.directory.join("recording-archives"),
    )
    .unwrap();
    assert!(archive_directory(&f.paths.directory).is_err());
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn large_directory_cleanup_inventories_all_names() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    let plugin = f.plugin();
    for i in 0..5000 {
        fs::write(f.paths.directory.join(format!("unrelated-{i}")), "").unwrap();
    }
    let inspected = scan(&f.paths.directory).unwrap();
    assert!(inspected.report.scan_complete);
    assert!(inspected.report.scanned_entries > 5000);
    let marker = f.file("jevia-events-", ".loss", b"retained");
    assert_eq!(cleanup(&f.paths, &f.storage, true).await.unwrap().moved, 0);
    assert!(plugin.exists());
    fs::remove_file(marker).unwrap();
    let report = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(report.moved, 1);
    assert!(!plugin.exists());
    assert!(report.archive.unwrap().is_dir());
}

#[test]
fn auxiliary_prefix_retains_only_valid_journal_ownership() {
    let id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        asset_prefix(
            "jevia-observer-",
            Path::new(&format!("jevia-events-{id}-tmp.jsonl"))
        ),
        format!("jevia-observer-{id}-")
    );
    assert_eq!(
        asset_prefix("jevia-observer-", Path::new("jevia-events-legacy.jsonl")),
        "jevia-observer-"
    );
}

#[tokio::test]
async fn cleanup_batches_jsonl_owners_but_checks_every_artifact() {
    for backend in local_backends() {
        let mut f = Fixture::new(backend).await;
        for _ in 0..2 {
            f.record.decision.run_id = uuid::Uuid::new_v4().to_string();
            f.storage.append(&f.record).await.unwrap();
            for index in 0..20 {
                if index % 2 == 0 {
                    f.file(
                        "jevia-observer-",
                        &format!("-{index}.mjs"),
                        include_bytes!("../observations/opencode.mjs"),
                    );
                } else {
                    let bytes = serde_json::to_vec(&serde_json::json!({"type":"snapshot", "event":f.record.execution.as_ref().unwrap().observations})).unwrap();
                    f.file(".jevia-event-checkpoint-", &format!("-{index}"), &bytes);
                }
            }
            // A matching earlier artifact must not bless a different later file.
            f.file("jevia-observer-", "-unknown.mjs", b"PRIVATE unknown plugin");
        }
        let preview = cleanup(&f.paths, &f.storage, false).await.unwrap();
        let reads = if matches!(f.storage, Storage::Jsonl(_)) {
            1
        } else {
            2
        };
        assert_eq!(preview.history_reads, reads);
        assert_eq!(preview.eligible, 40);
        assert_eq!(preview.retained["contents_not_known_saved"], 2);
        let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
        assert_eq!(applied.history_reads, reads);
        assert_eq!(applied.moved, 40);
        assert_eq!(applied.retained["contents_not_known_saved"], 2);
        assert_eq!(f.storage.check_deep().await.unwrap(), 2);
        let encoded = serde_json::to_string(&applied).unwrap();
        assert!(!encoded.contains("history_reads"));
        assert!(!encoded.contains("PRIVATE"));
    }
}

#[tokio::test]
async fn cached_lookup_failure_never_permits_cleanup() {
    let f = Fixture::new(StorageConfig::Jsonl).await;
    f.storage.append(&f.record).await.unwrap();
    let plugin = f.plugin();
    let checkpoint = f.checkpoint();
    use std::io::Write;
    let mut history = OpenOptions::new().append(true).open(&f.paths.runs).unwrap();
    history.write_all(b"PRIVATE corrupt tail\n").unwrap();
    for apply in [false, true] {
        let report = cleanup(&f.paths, &f.storage, apply).await.unwrap();
        assert_eq!(report.history_reads, 1);
        assert_eq!(report.moved, 0);
        assert_eq!(report.retained["history_unavailable_or_missing"], 2);
        assert!(plugin.exists() && checkpoint.exists());
    }
}

#[tokio::test]
async fn cleanup_bounds_batches_and_holds_each_selected_owner_lease() {
    let mut f = Fixture::new(StorageConfig::Jsonl).await;
    let mut ids = Vec::new();
    for index in 0..65 {
        f.record.decision.run_id = format!("00000000-0000-0000-0000-{index:012x}");
        ids.push(f.record.decision.run_id.clone());
        f.storage.append(&f.record).await.unwrap();
        f.plugin();
    }
    let preview = cleanup(&f.paths, &f.storage, false).await.unwrap();
    assert_eq!(preview.history_reads, 3);
    assert_eq!(preview.eligible, 65);
    let held = f.storage.execution_guard(&ids[10]).await.unwrap();
    let batch_ids = ids[..32].iter().cloned().collect();
    let batch = HistoryBatch::load(&f.storage, batch_ids, true).await;
    assert!(matches!(batch.saved[&ids[10]], Err("execution_busy")));
    assert!(f.storage.execution_guard(&ids[0]).await.is_err());
    assert_eq!(batch.saved.len(), 32);
    drop(batch);
    assert!(f.storage.execution_guard(&ids[0]).await.is_ok());
    let applied = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(applied.history_reads, 3);
    assert_eq!(applied.moved, 64);
    assert_eq!(applied.retained["execution_busy"], 1);
    drop(held);
    let last = cleanup(&f.paths, &f.storage, true).await.unwrap();
    assert_eq!(last.moved, 1);
    assert_eq!(last.history_reads, 1);
    assert_eq!(f.storage.check_deep().await.unwrap(), 65);
}
