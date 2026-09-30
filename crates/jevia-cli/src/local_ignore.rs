//! Preserve project rules while keeping private runtime artifacts out of Git.
use std::{
    fs::{self, OpenOptions, TryLockError},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

const RULES: &[&str] = &[
    ".gitignore.lock",
    "config.lock",
    "config-backups/",
    "*.db",
    "*.db-wal",
    "*.db-shm",
    "*.run-locks/",
    "run-leases/",
    "cache-leases/",
    "event-leases/",
    "history-backups/",
    "history-archives/",
    "runs.jsonl",
    "runs.lock",
    "cache.jsonl",
    "cache.lock",
    "jevia-events-*.jsonl",
    "jevia-events-*.loss",
    ".jevia-event-checkpoint-*",
    "jevia-observer-*.mjs",
    "*.tmp",
];

pub(crate) fn ensure(path: &Path) -> Result<()> {
    let parent = path.parent().context("ignore file has no directory")?;
    let lock_path = parent.join(".gitignore.lock");
    regular_or_missing(&lock_path)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(lock_path)?;
    if !file.metadata()?.is_file() {
        bail!("ignore lock is not a regular file");
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(TryLockError::WouldBlock) => bail!("project ignore rules are busy; retry later"),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    let _guard = crate::lease::FileLock::new(file);
    let metadata = regular_or_missing(path)?;
    let original = if metadata.is_some() {
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    let mut contents = original.clone().unwrap_or_default();
    for rule in RULES {
        if !contents.lines().any(|line| line == *rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(rule);
            contents.push('\n');
        }
    }
    if original.as_deref() == Some(contents.as_str()) {
        return Ok(());
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".jevia-ignore-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    if let Some(metadata) = metadata {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.as_file().sync_all()?;
    // The sidecar serializes Jevia writers; also refuse a detected external edit.
    let current = if regular_or_missing(path)?.is_some() {
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    if current != original {
        bail!("project ignore rules changed; refusing to overwrite them");
    }
    if original.is_some() {
        temporary.persist(path).map_err(|error| error.error)?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
    }
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn regular_or_missing(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata)),
        Ok(_) => bail!("ignore rules require regular files, not symlinks or directories"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_repairs_preserve_custom_rules_and_are_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".gitignore");
        let original = "# project rules\r\ncustom-secret\r\n!config.toml";
        fs::write(&path, original).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let path = &path;
                scope.spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        match ensure(path) {
                            Ok(()) => break,
                            Err(error)
                                if error
                                    .to_string()
                                    .starts_with("project ignore rules are busy")
                                    && Instant::now() < deadline =>
                            {
                                continue;
                            }
                            Err(error) => panic!("ignore repair failed: {error}"),
                        }
                    }
                });
            }
        });
        let repaired = fs::read_to_string(&path).unwrap();
        assert!(repaired.starts_with(original));
        for rule in RULES {
            assert_eq!(repaired.lines().filter(|line| line == rule).count(), 1);
        }
        ensure(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), repaired);
    }

    #[test]
    fn busy_or_invalid_ignore_files_are_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".gitignore");
        fs::write(&path, "custom\n").unwrap();
        let file = fs::File::create(directory.path().join(".gitignore.lock")).unwrap();
        file.lock().unwrap();
        let guard = crate::lease::FileLock::new(file);
        assert!(ensure(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom\n");
        drop(guard);
        fs::write(&path, [0xff]).unwrap();
        assert!(ensure(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
    }

    #[cfg(unix)]
    #[test]
    fn repairs_preserve_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".gitignore");
        fs::write(&path, "custom\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        ensure(&path).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ignore_and_lock_files_are_not_followed() {
        use std::os::unix::fs::symlink;
        for name in [".gitignore", ".gitignore.lock"] {
            let directory = tempfile::tempdir().unwrap();
            let other = directory.path().join("elsewhere");
            fs::write(&other, "keep me").unwrap();
            symlink(&other, directory.path().join(name)).unwrap();
            assert!(ensure(&directory.path().join(".gitignore")).is_err());
            assert_eq!(fs::read_to_string(other).unwrap(), "keep me");
        }
    }
}
