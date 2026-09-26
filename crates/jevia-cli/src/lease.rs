use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Stable sidecar locks are never unlinked: removing a locked inode could let
/// another process create a different lock for the same run.
pub fn try_acquire(directory: &Path, key: &str) -> Result<Option<File>> {
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
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error).context("could not acquire lease"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
