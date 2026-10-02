//! Bounded, private version probes own their descendants just like headless runs.
use super::*;
use tokio::io::AsyncReadExt;

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

pub async fn version_output(program: &str, root: &Path) -> Option<String> {
    // Cold native launchers can take longer than two seconds on a busy host.
    // Keep the probe bounded without treating ordinary startup as unsupported.
    output(program, &["--version"], root, Duration::from_secs(5)).await
}

async fn output(program: &str, args: &[&str], root: &Path, budget: Duration) -> Option<String> {
    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    });
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    let mut child = ProbeTree {
        tree: ProcessTree::new(command.spawn().ok()?),
        armed: true,
    };
    let stdout = child.tree.child.stdout().take()?;
    let read_and_wait = async {
        let mut bytes = Vec::new();
        stdout.take(257).read_to_end(&mut bytes).await.ok()?;
        if bytes.len() > 256 {
            return None;
        }
        if !child.tree.wait().await.ok()?.success() {
            return None;
        }
        String::from_utf8(bytes).ok()
    };
    if let Ok(Some(text)) = tokio::time::timeout(budget, read_and_wait).await {
        child.armed = false; // Both pipe EOF and process-tree completion confirmed.
        return Some(text);
    }
    // Do not accept partial output or leave a helper behind after a timeout,
    // failed exit, oversized output, or read error. Cleanup has its own bound.
    if child.tree.start_kill().is_ok()
        && matches!(
            tokio::time::timeout(Duration::from_secs(1), child.tree.wait()).await,
            Ok(Ok(_))
        )
    {
        child.armed = false;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_probe_accepts_success_and_rejects_failure_and_excess_output() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            version_output("rustc", dir.path())
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
            .is_none()
        );
        assert!(
            output(
                "rustc",
                &["--print", "cfg"],
                dir.path(),
                Duration::from_secs(5)
            )
            .await
            .is_none()
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
                .is_none()
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
