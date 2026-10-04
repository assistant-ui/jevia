//! Bounded, private version probes own their descendants just like headless runs.
use super::*;
use tokio::io::AsyncReadExt;

/// Fixed diagnostic categories only: never expose subprocess output or paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("executable not found")]
    NotFound,
    #[error("launch permission denied")]
    PermissionDenied,
    #[error("launch failed")]
    Spawn,
    #[error("output read failed")]
    Read,
    #[error("process wait failed")]
    Wait,
    #[error("nonzero exit")]
    Exit,
    #[error("startup deadline exceeded")]
    Deadline,
    #[error("output exceeded 256 bytes")]
    Oversized,
    #[error("output was not UTF-8")]
    Encoding,
    #[error("process cleanup unconfirmed")]
    Cleanup,
}

impl ProbeError {
    fn retryable(self) -> bool {
        matches!(self, Self::Read | Self::Wait | Self::Exit | Self::Deadline)
    }
}

struct ProbeTree {
    tree: ProcessTree,
    armed: bool,
}

impl Drop for ProbeTree {
    fn drop(&mut self) {
        // Covers cancellation of the future, including after the leader exits.
        // KillOnDrop alone only owns the direct child on Unix.
        if self.armed {
            let _ = self.tree.start_kill();
        }
    }
}

pub async fn version_output(
    program: &str,
    root: &Path,
    budget: Duration,
) -> Result<String, ProbeError> {
    // Cold native launchers can take longer than two seconds on a busy host.
    // At most two attempts, each with the original deadline plus bounded cleanup.
    // No persisted capability cache: replacing an executable must be rechecked.
    retry_output(program, &["--version"], root, budget).await
}

async fn retry_output(
    program: &str,
    args: &[&str],
    root: &Path,
    budget: Duration,
) -> Result<String, ProbeError> {
    match output(program, args, root, budget).await {
        Err(error) if error.retryable() => {
            eprintln!("jevia: native version probe retrying once: {error}; output redacted");
            output(program, args, root, budget).await
        }
        result => result,
    }
}

async fn output(
    program: &str,
    args: &[&str],
    root: &Path,
    budget: Duration,
) -> Result<String, ProbeError> {
    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        set_working_directory(command, root);
    });
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    let mut child = ProbeTree {
        tree: ProcessTree::new(command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ProbeError::NotFound,
            std::io::ErrorKind::PermissionDenied => ProbeError::PermissionDenied,
            _ => ProbeError::Spawn,
        })?),
        armed: true,
    };
    let read_and_wait = async {
        let stdout = child.tree.child.stdout().take().ok_or(ProbeError::Read)?;
        let mut bytes = Vec::new();
        stdout
            .take(257)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ProbeError::Read)?;
        if bytes.len() > 256 {
            return Err(ProbeError::Oversized);
        }
        if !child
            .tree
            .wait()
            .await
            .map_err(|_| ProbeError::Wait)?
            .success()
        {
            return Err(ProbeError::Exit);
        }
        String::from_utf8(bytes).map_err(|_| ProbeError::Encoding)
    };
    let error = match tokio::time::timeout(budget, read_and_wait).await {
        Ok(Ok(text)) => {
            child.armed = false; // Pipe EOF and process-tree completion confirmed.
            return Ok(text);
        }
        Ok(Err(error)) => error,
        Err(_) => ProbeError::Deadline,
    };
    // Do not accept partial output or leave a helper behind after a timeout,
    // failed exit, oversized output, or read error. Cleanup has its own bound.
    if child.tree.start_kill().is_ok()
        && matches!(
            tokio::time::timeout(Duration::from_secs(1), child.tree.wait()).await,
            Ok(Ok(_))
        )
    {
        child.armed = false;
        return Err(error);
    }
    // A second attempt must not overlap a possibly still-running first probe.
    Err(ProbeError::Cleanup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_excludes_invalid_output_launch_errors_and_unconfirmed_cleanup() {
        for error in [
            ProbeError::NotFound,
            ProbeError::PermissionDenied,
            ProbeError::Spawn,
            ProbeError::Oversized,
            ProbeError::Encoding,
            ProbeError::Cleanup,
        ] {
            assert!(!error.retryable(), "{error}");
        }
        for error in [
            ProbeError::Read,
            ProbeError::Wait,
            ProbeError::Exit,
            ProbeError::Deadline,
        ] {
            assert!(error.retryable(), "{error}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transient_exit_or_deadline_recovers_once_without_reusing_partial_output() {
        for failure in ["exit 7", "printf stale; sleep 10"] {
            let dir = tempfile::tempdir().unwrap();
            let script = format!(
                "printf x >> attempts; if test -f warm; then printf fresh; else touch warm; {failure}; fi"
            );
            assert_eq!(
                retry_output(
                    "sh",
                    &["-c", &script],
                    dir.path(),
                    Duration::from_millis(200)
                )
                .await,
                Ok("fresh".into())
            );
            assert_eq!(
                std::fs::read_to_string(dir.path().join("attempts")).unwrap(),
                "xx"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permanent_failures_are_bounded_and_keep_specific_diagnostics() {
        for (script, expected, attempts) in [
            ("printf x >> attempts; exit 7", ProbeError::Exit, "xx"),
            ("printf x >> attempts; sleep 10", ProbeError::Deadline, "xx"),
            (
                "printf x >> attempts; printf '\\377'",
                ProbeError::Encoding,
                "x",
            ),
            (
                "printf x >> attempts; printf '%0300d' 0; exec sleep 10",
                ProbeError::Oversized,
                "x",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let start = std::time::Instant::now();
            assert_eq!(
                retry_output(
                    "sh",
                    &["-c", script],
                    dir.path(),
                    Duration::from_millis(200)
                )
                .await,
                Err(expected)
            );
            assert!(start.elapsed() < Duration::from_secs(4));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("attempts")).unwrap(),
                attempts
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_retry_does_not_leave_descendants_or_start_a_third_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let script = "printf x >> attempts; if test -f warm; then (sleep 1; touch escaped) & sleep 10; else touch warm; exit 7; fi";
        assert!(
            tokio::time::timeout(
                Duration::from_millis(200),
                retry_output("sh", &["-c", script], dir.path(), Duration::from_secs(5))
            )
            .await
            .is_err()
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!dir.path().join("escaped").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("attempts")).unwrap(),
            "xx"
        );
    }

    #[tokio::test]
    async fn native_probe_accepts_success_and_rejects_failure_and_excess_output() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            version_output("rustc", dir.path(), Duration::from_secs(5))
                .await
                .unwrap()
                .starts_with("rustc ")
        );
        assert!(
            output(
                "rustc",
                &["--definitely-invalid"],
                dir.path(),
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
        assert!(
            output(
                "rustc",
                &["--print", "cfg"],
                dir.path(),
                Duration::from_secs(5)
            )
            .await
            .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_descendants_with_and_without_inherited_stdout() {
        for script in [
            "(sleep 1; touch escaped) & printf 'version'; exit 0",
            "(sleep 1; touch escaped) >/dev/null & printf 'version'; exit 0",
            "(sleep 1; touch escaped) & sleep 10",
        ] {
            let dir = tempfile::tempdir().unwrap();
            assert!(
                output(
                    "sh",
                    &["-c", script],
                    dir.path(),
                    Duration::from_millis(100)
                )
                .await
                .is_err()
            );
            tokio::time::sleep(Duration::from_millis(1100)).await;
            assert!(!dir.path().join("escaped").exists());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_probe_future_stops_its_owned_group() {
        let dir = tempfile::tempdir().unwrap();
        let probe = output(
            "sh",
            &["-c", "(sleep 1; touch escaped) & sleep 10"],
            dir.path(),
            Duration::from_secs(10),
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), probe)
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!dir.path().join("escaped").exists());
    }
}
