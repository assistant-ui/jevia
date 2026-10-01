use super::*;
use crate::storage::ExecutionGuard;

#[derive(Default)]
pub(super) struct HistoryBatch {
    pub saved: BTreeMap<String, std::result::Result<HarnessObservations, &'static str>>,
    guards: Vec<ExecutionGuard>,
    #[cfg(test)]
    pub reads: usize,
}

impl HistoryBatch {
    pub async fn load(storage: &Storage, ids: BTreeSet<String>, apply: bool) -> Self {
        let mut batch = Self::default();
        let mut available = BTreeSet::new();
        for id in ids {
            if apply {
                match storage.execution_guard(&id).await {
                    Ok(guard) => batch.guards.push(guard),
                    Err(_) => {
                        batch.saved.insert(id, Err("execution_busy"));
                        continue;
                    }
                }
            }
            available.insert(id);
        }
        if available.is_empty() {
            return batch;
        }
        match storage {
            Storage::Jsonl(paths) => {
                let path = paths.runs.clone();
                let ids = available.clone();
                let records =
                    tokio::task::spawn_blocking(move || crate::store::try_get_many(&path, &ids))
                        .await;
                #[cfg(test)]
                {
                    batch.reads += 1;
                }
                let mut records = match records {
                    Ok(Ok(records)) => records,
                    _ => {
                        for id in available {
                            batch
                                .saved
                                .insert(id, Err("history_unavailable_or_missing"));
                        }
                        return batch;
                    }
                };
                for id in available {
                    let result = records
                        .remove(&id)
                        .ok_or("history_unavailable_or_missing")
                        .and_then(saved_observations);
                    batch.saved.insert(id, result);
                }
            }
            Storage::Database(_) => {
                for id in available {
                    let result = storage
                        .get_for_replay(&id)
                        .await
                        .map_err(|_| "history_unavailable_or_missing")
                        .and_then(saved_observations);
                    #[cfg(test)]
                    {
                        batch.reads += 1;
                    }
                    batch.saved.insert(id, result);
                }
            }
        }
        batch
    }
}
