//! Bounded acquisition of stable history/cache sidecars; never unlink a lock.
use std::{
    fs::{File, TryLockError},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};

pub(crate) fn acquire(
    file: File,
    shared: bool,
    budget: Duration,
    resource: &str,
) -> Result<crate::lease::FileLock> {
    let deadline = Instant::now() + budget;
    loop {
        let result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        match result {
            Ok(()) => return Ok(crate::lease::FileLock::new(file)),
            Err(TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    bail!(
                        "{resource} busy; file-lock wait expired; retry after the other operation finishes"
                    );
                }
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_modes_time_out_without_changing_or_stealing_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.lock");
        std::fs::write(&path, "keep").unwrap();
        let open = || File::options().read(true).write(true).open(&path).unwrap();
        let owner = open();
        owner.lock().unwrap();
        for shared in [false, true] {
            let start = Instant::now();
            let error = acquire(open(), shared, Duration::from_millis(20), "fixture").unwrap_err();
            assert!(error.to_string().contains("file-lock wait expired"));
            assert!(start.elapsed() < Duration::from_secs(1));
            assert!(matches!(open().try_lock(), Err(TryLockError::WouldBlock)));
        }
        owner.unlock().unwrap();
        // Windows mandatory file locks also deny reads through other handles.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        drop(acquire(open(), false, Duration::ZERO, "fixture").unwrap());
        acquire(open(), true, Duration::ZERO, "fixture").unwrap();
    }
}
