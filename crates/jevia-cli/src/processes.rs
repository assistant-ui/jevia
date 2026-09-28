use anyhow::{Context, Result};
use jevia_core::RunState;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    future::Future,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

#[derive(Debug)]
pub enum ProcessResult {
    Exited(ExitStatus),
    Stopped { state: RunState, launched: bool },
}

struct ProcessTree {
    child: Box<dyn ChildWrapper>,
    #[cfg(unix)]
    group: nix::unistd::Pid,
}

impl ProcessTree {
    fn new(child: Box<dyn ChildWrapper>) -> Self {
        Self {
            #[cfg(unix)]
            group: nix::unistd::Pid::from_raw(
                child.id().expect("newly spawned child has a PID") as i32
            ),
            child,
        }
    }

    async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        // waitpid can only reap our children. On Unix, grandchildren can be
        // reparented outside Jevia when the group leader exits, so a successful
        // wrapped wait does not establish that the owned group is empty.
        // Keep this wait inside the timeout/cancellation select, including after
        // the leader's status has been cached by process-wrap.
        #[cfg(unix)]
        loop {
            match nix::sys::signal::killpg(self.group, None) {
                Err(nix::errno::Errno::ESRCH) => break,
                Err(error) => return Err(error.into()),
                Ok(()) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        Ok(status)
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        let result = self.child.start_kill();
        // The last group member can exit between selecting cancellation/the
        // deadline and sending SIGKILL. An absent group is already stopped.
        #[cfg(unix)]
        if result.as_ref().err().and_then(std::io::Error::raw_os_error)
            == Some(nix::errno::Errno::ESRCH as i32)
        {
            return Ok(());
        }
        result
    }
}

pub struct Runner {
    supervised: bool,
    #[cfg(unix)]
    signals: Option<(tokio::signal::unix::Signal, tokio::signal::unix::Signal)>,
    #[cfg(windows)]
    signals: Option<tokio::signal::windows::CtrlC>,
}

impl Runner {
    pub fn new(supervised: bool) -> Result<Self> {
        #[cfg(unix)]
        let signals = if supervised {
            Some((
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
            ))
        } else {
            None
        };
        #[cfg(windows)]
        let signals = if supervised {
            Some(tokio::signal::windows::ctrl_c()?)
        } else {
            None
        };
        Ok(Self {
            supervised,
            signals,
        })
    }

    pub async fn run(
        &mut self,
        program: &str,
        args: &[String],
        root: &Path,
        timeout: Option<Duration>,
    ) -> Result<ProcessResult> {
        if !self.supervised {
            return Ok(ProcessResult::Exited(
                tokio::process::Command::new(program)
                    .args(args)
                    .current_dir(root)
                    .status()
                    .await?,
            ));
        }
        let signals = self
            .signals
            .as_mut()
            .expect("supervised signals initialized");
        let cancel = async {
            #[cfg(unix)]
            tokio::select! {
                _ = signals.0.recv() => Ok(()),
                _ = signals.1.recv() => Ok(()),
            }
            #[cfg(windows)]
            {
                signals.recv().await;
                Ok(())
            }
        };
        supervise(program, args, root, timeout, cancel).await
    }
}

async fn supervise(
    program: &str,
    args: &[String],
    root: &Path,
    timeout: Option<Duration>,
    cancel: impl Future<Output = std::io::Result<()>>,
) -> Result<ProcessResult> {
    tokio::pin!(cancel);
    tokio::select! {
        biased;
        result = &mut cancel => {
            result.context("cancellation listener failed before launch")?;
            return Ok(ProcessResult::Stopped { state: RunState::Cancelled, launched: false });
        }
        _ = std::future::ready(()) => {}
    }
    let mut command = CommandWrap::with_new(program, |command| {
        command.args(args).current_dir(root).stdin(Stdio::null());
    });
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    let mut child = ProcessTree::new(
        command
            .spawn()
            .context("could not start supervised process")?,
    );
    let deadline = async {
        match timeout {
            Some(duration) => tokio::time::sleep(duration).await,
            None => std::future::pending().await,
        }
    };
    let (state, signal_error) = tokio::select! {
        biased;
        status = child.wait() => match status {
            Ok(status) => return Ok(ProcessResult::Exited(status)),
            Err(error) => (RunState::Interrupted, Some(error)),
        },
        result = &mut cancel => (RunState::Cancelled, result.err()),
        _ = deadline => (RunState::TimedOut, None),
    };
    // Kill the owned process group/job, not merely the direct child, then reap.
    if let Err(error) = child.start_kill() {
        eprintln!("jevia: process-tree cleanup failed: {error}; inspect surviving processes");
        return Ok(ProcessResult::Stopped {
            state: RunState::Interrupted,
            launched: true,
        });
    }
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), child.wait()).await,
        Ok(Ok(_))
    ) {
        eprintln!("jevia: process cleanup could not be confirmed; inspect surviving processes");
        return Ok(ProcessResult::Stopped {
            state: RunState::Interrupted,
            launched: true,
        });
    }
    if let Some(error) = signal_error {
        eprintln!("jevia: process supervision failed: {error}");
        return Ok(ProcessResult::Stopped {
            state: RunState::Interrupted,
            launched: true,
        });
    }
    Ok(ProcessResult::Stopped {
        state,
        launched: true,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parent_exit_waits_for_background_children_and_preserves_status() {
        for code in [0, 7] {
            let dir = tempfile::tempdir().unwrap();
            let result = supervise(
                "sh",
                &[
                    "-c".into(),
                    format!("(sleep 0.2; touch finished) & exit {code}"),
                ],
                dir.path(),
                Some(Duration::from_secs(5)),
                std::future::pending(),
            )
            .await
            .unwrap();
            assert!(matches!(result, ProcessResult::Exited(status) if status.code() == Some(code)));
            assert!(dir.path().join("finished").exists());
        }
    }

    #[tokio::test]
    async fn timeout_still_stops_children_after_parent_exit() {
        let dir = tempfile::tempdir().unwrap();
        let result = supervise(
            "sh",
            &["-c".into(), "(sleep 1; touch escaped) & exit 0".into()],
            dir.path(),
            Some(Duration::from_millis(100)),
            std::future::pending(),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            ProcessResult::Stopped {
                state: RunState::TimedOut,
                launched: true
            }
        ));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!dir.path().join("escaped").exists());
    }

    #[tokio::test]
    async fn cancellation_still_stops_children_after_parent_exit() {
        let dir = tempfile::tempdir().unwrap();
        let result = supervise(
            "sh",
            &["-c".into(), "(sleep 1; touch escaped) & exit 0".into()],
            dir.path(),
            None,
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            ProcessResult::Stopped {
                state: RunState::Cancelled,
                launched: true
            }
        ));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!dir.path().join("escaped").exists());
    }

    #[tokio::test]
    async fn timeout_stops_descendants_before_they_can_write() {
        let dir = tempfile::tempdir().unwrap();
        let result = supervise(
            "sh",
            &["-c".into(), "(sleep 1; touch escaped) & wait".into()],
            dir.path(),
            Some(Duration::from_millis(100)),
            std::future::pending(),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            ProcessResult::Stopped {
                state: RunState::TimedOut,
                launched: true
            }
        ));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!dir.path().join("escaped").exists());
    }
    #[tokio::test]
    async fn cancellation_reaps_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let result = supervise("sleep", &["10".into()], dir.path(), None, async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(())
        })
        .await
        .unwrap();
        assert!(matches!(
            result,
            ProcessResult::Stopped {
                state: RunState::Cancelled,
                launched: true
            }
        ));
    }
}
