use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Poll only the small coordination lock while waiting, never history or cache.
/// The caller must reload evidence after acquisition or expiry, before routing.
pub async fn acquire_until(
    directory: &Path,
    key: &str,
    deadline: Instant,
) -> Result<(Option<FileLock>, bool)> {
    let mut waited = false;
    let mut delay = Duration::from_millis(25);
    loop {
        if Instant::now() >= deadline {
            return Ok((None, waited));
        }
        if let Some(guard) = try_acquire(directory, key)? {
            return Ok((Some(guard), waited));
        }
        waited = true;
        tokio::time::sleep(delay.min(deadline.saturating_duration_since(Instant::now()))).await;
        delay = next_delay(delay);
    }
}

fn next_delay(delay: Duration) -> Duration {
    delay.saturating_mul(2).min(Duration::from_millis(250))
}

/// Owns an already-acquired lock without transferring ownership to duplicated
/// descriptors (for example, those inherited during a concurrent Unix fork).
#[derive(Debug)]
pub(crate) struct FileLock(File);

impl FileLock {
    pub(crate) fn new(file: File) -> Self {
        Self(file)
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Close-on-exec alone leaves a window before the child execs. Explicitly
        // release this operation's lock; closing remains a fallback on error.
        let _ = self.0.unlock();
    }
}

/// Stable sidecar locks are never unlinked: removing a locked inode could let
/// another process create a different lock for the same run.
pub fn try_acquire(directory: &Path, key: &str) -> Result<Option<FileLock>> {
    fs::create_dir_all(directory).context("could not create lease directory")?;
    let name = Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(directory.join(format!("{name}.lock")))
        .context("could not open lease")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock::new(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error).context("could not acquire lease"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordination_backoff_is_capped() {
        let mut delay = Duration::from_millis(25);
        for expected in [50, 100, 200, 250, 250] {
            delay = next_delay(delay);
            assert_eq!(delay, Duration::from_millis(expected));
        }
    }

    #[tokio::test]
    async fn coordination_wait_releases_on_cancellation_and_respects_expiry() {
        let directory = tempfile::tempdir().unwrap();
        let guard = try_acquire(directory.path(), "stripe").unwrap().unwrap();
        let mut waiter = Box::pin(acquire_until(
            directory.path(),
            "stripe",
            Instant::now() + Duration::from_secs(5),
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiter)
                .await
                .is_err()
        );
        drop(guard);
        let (guard, waited) = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(waited);
        assert!(guard.is_some());
        drop(guard);
        let (expired, _) = acquire_until(directory.path(), "stripe", Instant::now())
            .await
            .unwrap();
        assert!(expired.is_none());
        let guard = try_acquire(directory.path(), "stripe").unwrap().unwrap();
        let waiter = acquire_until(
            directory.path(),
            "stripe",
            Instant::now() + Duration::from_secs(5),
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), waiter)
                .await
                .is_err()
        );
        drop(guard);
        assert!(try_acquire(directory.path(), "stripe").unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn lease_releases_while_a_duplicate_descriptor_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let first = try_acquire(dir.path(), "run").unwrap().unwrap();
        let duplicate = first.0.try_clone().unwrap();
        drop(first);
        let next = try_acquire(dir.path(), "run").unwrap();
        assert!(
            next.is_some(),
            "an inherited descriptor must not retain ownership"
        );
        drop(duplicate);
        assert!(try_acquire(dir.path(), "run").unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn shared_and_exclusive_locks_release_without_waiting_for_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.lock");
        for shared in [true, false] {
            let file = File::options()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            if shared {
                file.lock_shared().unwrap();
            } else {
                file.lock().unwrap();
            }
            let guard = FileLock::new(file);
            let duplicate = guard.0.try_clone().unwrap();
            let contender = File::options().read(true).write(true).open(&path).unwrap();
            assert!(matches!(
                contender.try_lock(),
                Err(TryLockError::WouldBlock)
            ));
            drop(guard);
            contender.try_lock().unwrap();
            drop(duplicate);
            let another = File::options().read(true).write(true).open(&path).unwrap();
            assert!(matches!(another.try_lock(), Err(TryLockError::WouldBlock)));
            contender.unlock().unwrap();
        }
    }

    #[test]
    fn lease_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let first = try_acquire(dir.path(), "../untrusted/run")
            .unwrap()
            .unwrap();
        assert!(
            try_acquire(dir.path(), "../untrusted/run")
                .unwrap()
                .is_none()
        );
        assert!(try_acquire(dir.path(), "different-run").unwrap().is_some());
        drop(first);
        assert!(
            try_acquire(dir.path(), "../untrusted/run")
                .unwrap()
                .is_some()
        );
    }
}
