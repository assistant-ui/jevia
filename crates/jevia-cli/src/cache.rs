use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use jevia_core::RouteDecision;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    schema_version: u32,
    key: String,
    decision: RouteDecision,
    created_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub total: usize,
    pub active: usize,
    pub expired: usize,
}

pub fn lookup(path: &Path, key: &str) -> Result<Option<RouteDecision>> {
    lookup_at(path, key, now_ms())
}

pub fn insert(
    path: &Path,
    key: String,
    decision: RouteDecision,
    ttl_seconds: u64,
    max_entries: usize,
) -> Result<()> {
    if max_entries == 0 {
        bail!("cache max_entries must be greater than zero");
    }
    let created_at_ms = now_ms();
    let expires_at_ms = created_at_ms.saturating_add(ttl_seconds.saturating_mul(1_000));
    insert_at(
        path,
        CacheEntry {
            schema_version: CACHE_SCHEMA_VERSION,
            key,
            decision,
            created_at_ms,
            expires_at_ms,
        },
        max_entries,
    )
}

pub fn stats(path: &Path) -> Result<CacheStats> {
    let entries = load(path)?;
    let now = now_ms();
    let active = entries
        .iter()
        .filter(|entry| entry.expires_at_ms > now)
        .count();
    Ok(CacheStats {
        total: entries.len(),
        active,
        expired: entries.len().saturating_sub(active),
    })
}

pub fn clear(path: &Path) -> Result<bool> {
    let parent = path
        .parent()
        .context("cache path does not have a parent directory")?;
    if !parent.exists() {
        return Ok(false);
    }

    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    match fs::remove_file(path) {
        Ok(()) => {
            sync_parent(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not clear {}", path.display())),
    }
}

fn lookup_at(path: &Path, key: &str, now: u64) -> Result<Option<RouteDecision>> {
    let entries = load(path)?;
    Ok(entries
        .into_iter()
        .rev()
        .find(|entry| entry.key == key && entry.expires_at_ms > now)
        .map(|entry| entry.decision))
}

fn insert_at(path: &Path, entry: CacheEntry, max_entries: usize) -> Result<()> {
    entry.decision.validate().map_err(anyhow::Error::msg)?;
    let parent = path
        .parent()
        .context("cache path does not have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let _lock = acquire_lock(path, LockMode::Exclusive)?;
    let mut entries = load_unlocked(path)?;
    entries.retain(|existing| {
        existing.key != entry.key && existing.expires_at_ms > entry.created_at_ms
    });
    entries.push(entry);
    entries.sort_by_key(|entry| entry.created_at_ms);
    let excess = entries.len().saturating_sub(max_entries);
    if excess > 0 {
        entries.drain(..excess);
    }
    write_unlocked(path, &entries)
}

fn load(path: &Path) -> Result<Vec<CacheEntry>> {
    let parent = path
        .parent()
        .context("cache path does not have a parent directory")?;
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let _lock = acquire_lock(path, LockMode::Shared)?;
    load_unlocked(path)
}

fn load_unlocked(path: &Path) -> Result<Vec<CacheEntry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not open cache at {}", path.display()));
        }
    };

    let mut entries = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!("could not read line {} from {}", index + 1, path.display())
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: CacheEntry = serde_json::from_str(&line)
            .map_err(|error| crate::diagnostics::json_line("cache entry", index + 1, &error))?;
        if entry.schema_version != CACHE_SCHEMA_VERSION {
            bail!(
                "unsupported cache schema {} on line {} of {}",
                entry.schema_version,
                index + 1,
                path.display()
            );
        }
        entry
            .decision
            .validate()
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "invalid cached decision on line {} (contents redacted)",
                    index + 1
                )
            })?;
        entries.push(entry);
    }
    Ok(entries)
}

fn write_unlocked(path: &Path, entries: &[CacheEntry]) -> Result<()> {
    let parent = path
        .parent()
        .context("cache path does not have a parent directory")?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("could not create a temporary file in {}", parent.display()))?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        for entry in entries {
            serde_json::to_writer(&mut writer, entry).context("could not encode cache entry")?;
            writer
                .write_all(b"\n")
                .context("could not terminate cache entry")?;
        }
        writer.flush().context("could not flush routing cache")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("could not sync routing cache")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not atomically replace {}", path.display()))?;
    sync_parent(path)
}

#[derive(Debug, Clone, Copy)]
enum LockMode {
    Shared,
    Exclusive,
}

fn acquire_lock(path: &Path, mode: LockMode) -> Result<crate::lease::FileLock> {
    let lock_path = path.with_extension("lock");
    let lock = private_lock_options()
        .open(&lock_path)
        .with_context(|| format!("could not open cache lock at {}", lock_path.display()))?;
    match mode {
        LockMode::Shared => lock.lock_shared(),
        LockMode::Exclusive => lock.lock(),
    }
    .with_context(|| format!("could not acquire cache lock at {}", lock_path.display()))?;
    Ok(crate::lease::FileLock::new(lock))
}

fn private_lock_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("cache path does not have a parent directory")?;
    File::open(parent)
        .with_context(|| format!("could not open {} for syncing", parent.display()))?
        .sync_all()
        .with_context(|| format!("could not sync {}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Barrier},
        thread,
    };

    use jevia_core::{Config, DecisionSource, RouteDecision, route_cache_key};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn cache_hits_respect_expiration() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("cache.jsonl");
        insert_at(&path, entry("key", 100, 200), 10).expect("cache entry inserts");

        assert!(lookup_at(&path, "key", 199).expect("cache reads").is_some());
        assert!(lookup_at(&path, "key", 200).expect("cache reads").is_none());
    }

    #[test]
    fn cache_evicts_the_oldest_entries() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("cache.jsonl");
        insert_at(&path, entry("first", 100, 1_000), 2).expect("first entry inserts");
        insert_at(&path, entry("second", 200, 1_000), 2).expect("second entry inserts");
        insert_at(&path, entry("third", 300, 1_000), 2).expect("third entry inserts");

        assert!(
            lookup_at(&path, "first", 400)
                .expect("cache reads")
                .is_none()
        );
        assert!(
            lookup_at(&path, "second", 400)
                .expect("cache reads")
                .is_some()
        );
        assert!(
            lookup_at(&path, "third", 400)
                .expect("cache reads")
                .is_some()
        );
    }

    #[test]
    fn concurrent_inserts_preserve_every_entry() {
        const WRITERS: usize = 8;

        let directory = tempdir().expect("temporary directory");
        let path = Arc::new(directory.path().join("cache.jsonl"));
        let barrier = Arc::new(Barrier::new(WRITERS));
        let writers: Vec<_> = (0..WRITERS)
            .map(|index| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    insert_at(
                        &path,
                        entry(&format!("key-{index}"), index as u64 + 1, 10_000),
                        WRITERS,
                    )
                    .expect("concurrent insert succeeds");
                })
            })
            .collect();

        for writer in writers {
            writer.join().expect("writer does not panic");
        }

        assert_eq!(load(&path).expect("cache loads").len(), WRITERS);
    }

    #[test]
    fn cache_file_contains_no_task_text() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("cache.jsonl");
        let task = "private task text";
        let key =
            route_cache_key(task, None, &Config::default(), &[]).expect("cache key is created");
        insert_at(&path, entry(&key, 100, 1_000), 10).expect("entry inserts");

        let persisted = fs::read_to_string(path).expect("cache is readable");
        assert!(!persisted.contains(task));
        assert!(persisted.contains(&key));
    }

    #[test]
    fn clear_removes_decisions_but_keeps_the_cache_usable() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("cache.jsonl");
        insert_at(&path, entry("key", 100, 1_000), 10).expect("entry inserts");

        assert!(clear(&path).expect("cache clears"));
        assert_eq!(stats(&path).expect("cache remains usable").total, 0);
        assert!(!clear(&path).expect("empty cache clears safely"));
    }

    #[test]
    fn malformed_cache_is_reported_without_rewriting_it() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("cache.jsonl");
        let malformed = "{not valid json}\n";
        fs::write(&path, malformed).expect("malformed cache is written");

        let error = stats(&path).expect_err("malformed cache is rejected");

        assert!(error.to_string().contains("invalid cache entry on line 1"));
        assert_eq!(
            fs::read_to_string(path).expect("cache remains readable"),
            malformed
        );
    }

    #[test]
    fn invalid_decisions_cannot_be_inserted_or_reused() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("cache.jsonl");
        let mut invalid = entry("key", 0, 1000);
        invalid
            .decision
            .probabilities
            .insert("private-tier".into(), 2.0);
        assert!(insert_at(&path, invalid.clone(), 10).is_err());
        assert!(!path.exists());
        // Simulate a cache written by an older version; the caller treats this
        // error as a cache miss and asks the provider again.
        fs::write(&path, serde_json::to_string(&invalid).unwrap()).unwrap();
        let error = lookup_at(&path, "key", 1).unwrap_err();
        assert!(!format!("{error:#}").contains("private-tier"));
    }

    fn entry(key: &str, created_at_ms: u64, expires_at_ms: u64) -> CacheEntry {
        CacheEntry {
            schema_version: CACHE_SCHEMA_VERSION,
            key: key.to_owned(),
            decision: decision(),
            created_at_ms,
            expires_at_ms,
        }
    }

    fn decision() -> RouteDecision {
        RouteDecision {
            run_id: "cached-run".to_owned(),
            tier: "balanced".to_owned(),
            suggested_tier: "balanced".to_owned(),
            confidence: 0.8,
            probabilities: BTreeMap::new(),
            fallback_applied: false,
            jev_model: "jev-test".to_owned(),
            created_at_ms: 1,
            source: DecisionSource::Live,
        }
    }
}
