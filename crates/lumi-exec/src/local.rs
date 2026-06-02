//! The local-host execution backend.

use async_trait::async_trait;
use lumi_core::{ExecError, ExecOutput, ExecRequest, Executor};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// Runs commands and file operations directly on the host filesystem.
pub struct LocalExecutor {
    working_dir: PathBuf,
}

impl LocalExecutor {
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        LocalExecutor { working_dir: working_dir.into() }
    }

    fn shell() -> (&'static str, &'static str) {
        if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("/bin/sh", "-c")
        }
    }
}

enum Outcome {
    Cancelled,
    /// (exit status if the process completed, timed_out flag)
    Finished(Option<std::process::ExitStatus>, bool),
}

#[async_trait]
impl Executor for LocalExecutor {
    async fn exec(&self, req: ExecRequest, ct: CancellationToken) -> Result<ExecOutput, ExecError> {
        let (sh, flag) = Self::shell();
        let cwd = req
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.working_dir.clone());

        let mut cmd = Command::new(sh);
        cmd.arg(flag)
            .arg(&req.command)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &req.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(ExecError::from)?;

        // Drain stdout/stderr concurrently so a chatty process can't deadlock
        // on a full pipe while we wait.
        let mut stdout_pipe = child.stdout.take().expect("piped stdout");
        let mut stderr_pipe = child.stderr.take().expect("piped stderr");
        let out_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf).await;
            buf
        });
        let err_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf).await;
            buf
        });

        let timeout = req.timeout;
        let outcome = {
            let wait = async {
                match timeout {
                    Some(d) => match tokio::time::timeout(d, child.wait()).await {
                        Ok(r) => r.map(|st| (Some(st), false)),
                        Err(_) => Ok((None, true)),
                    },
                    None => child.wait().await.map(|st| (Some(st), false)),
                }
            };
            tokio::pin!(wait);
            tokio::select! {
                _ = ct.cancelled() => Outcome::Cancelled,
                r = &mut wait => {
                    let (st, timed_out) = r.map_err(ExecError::from)?;
                    Outcome::Finished(st, timed_out)
                }
            }
        };

        let (exit_code, timed_out) = match outcome {
            Outcome::Cancelled => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(ExecError::Cancelled);
            }
            Outcome::Finished(Some(status), false) => {
                (status.code().unwrap_or(-1), false)
            }
            Outcome::Finished(_, _timed_out) => {
                // timed out: ensure the process is gone
                let _ = child.start_kill();
                let _ = child.wait().await;
                (-1, true)
            }
        };

        let stdout = String::from_utf8_lossy(&out_task.await.unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err_task.await.unwrap_or_default()).into_owned();
        Ok(ExecOutput { stdout, stderr, exit_code, timed_out })
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ExecError> {
        tokio::fs::read(path).await.map_err(ExecError::from)
    }

    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<(), ExecError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(ExecError::from)?;
        }
        tokio::fs::write(path, contents).await.map_err(ExecError::from)
    }

    async fn exists(&self, path: &Path) -> Result<bool, ExecError> {
        tokio::fs::try_exists(path).await.map_err(ExecError::from)
    }

    fn working_dir(&self) -> &Path {
        &self.working_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn runs_command_and_captures_output() {
        let exec = LocalExecutor::new(std::env::temp_dir());
        let out = exec
            .exec(ExecRequest::new("echo hello"), CancellationToken::new())
            .await
            .unwrap();
        assert!(out.success());
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported() {
        let exec = LocalExecutor::new(std::env::temp_dir());
        let out = exec
            .exec(ExecRequest::new("exit 3"), CancellationToken::new())
            .await
            .unwrap();
        assert!(!out.success());
        assert_eq!(out.exit_code, 3);
    }

    #[tokio::test]
    async fn times_out_long_command() {
        let exec = LocalExecutor::new(std::env::temp_dir());
        let out = exec
            .exec(
                ExecRequest::new("sleep 5").timeout(Duration::from_millis(150)),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.timed_out);
    }

    #[tokio::test]
    async fn cancellation_kills_command() {
        let exec = LocalExecutor::new(std::env::temp_dir());
        let ct = CancellationToken::new();
        let ct2 = ct.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            ct2.cancel();
        });
        let res = exec.exec(ExecRequest::new("sleep 5"), ct).await;
        assert!(matches!(res, Err(ExecError::Cancelled)));
    }

    #[tokio::test]
    async fn reads_and_writes_files() {
        let dir = tempfile::tempdir().unwrap();
        let exec = LocalExecutor::new(dir.path());
        let f = dir.path().join("nested/hello.txt");
        exec.write_file(&f, b"hi there").await.unwrap();
        assert!(exec.exists(&f).await.unwrap());
        let data = exec.read_file(&f).await.unwrap();
        assert_eq!(&data, b"hi there");
    }
}
