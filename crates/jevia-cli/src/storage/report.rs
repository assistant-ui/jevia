//! Versioned, payload-free storage diagnostics for automation. Never initialize.
use super::{ObservationIndexStatus, Storage};
use crate::paths::ProjectPaths;
use jevia_core::StorageConfig;
use serde::Serialize;

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    backend: Option<&'static str>,
    check: &'static str,
    pub ok: bool,
    records: Option<usize>,
    passive_history_index: Option<ObservationIndexStatus>,
    error: Option<Failure>,
}

#[derive(Serialize)]
struct Failure {
    code: &'static str,
    message: &'static str,
}

impl Report {
    fn failed(mut self, code: &'static str, message: &'static str) -> Self {
        self.error = Some(Failure { code, message });
        self
    }
}

pub async fn inspect(deep: bool) -> Report {
    let mut report = Report {
        schema_version: 1,
        backend: None,
        check: if deep { "deep" } else { "basic" },
        ok: false,
        records: None,
        passive_history_index: None,
        error: None,
    };
    let config = ProjectPaths::discover()
        .and_then(|paths| crate::load_config(&paths).map(|config| (paths, config)));
    let Ok((paths, config)) = config else {
        return report.failed(
            "configuration_unavailable",
            "Project configuration is missing, unreadable, or invalid.",
        );
    };
    report.backend = Some(match &config.storage {
        StorageConfig::Jsonl => "jsonl",
        StorageConfig::Sqlite { .. } => "sqlite",
        StorageConfig::Postgres { .. } => "postgres",
    });
    let Ok(storage) = Storage::open(&config, &paths, false).await else {
        return report.failed("storage_unavailable", "Storage could not be opened; inspect configuration, credentials, connectivity, and initialization.");
    };
    let count = if deep {
        storage.check_deep().await
    } else {
        storage.check().await
    };
    let Ok(count) = count else {
        return report.failed("history_check_failed", "Storage health check failed; inspect history and permissions. No repair was attempted.");
    };
    report.records = Some(count);
    let Ok(index) = storage.observation_index_status().await else {
        return report.failed(
            "index_check_failed",
            "Index metadata could not be inspected. No index was created or replaced.",
        );
    };
    report.passive_history_index = Some(index);
    report.ok = true;
    report
}
