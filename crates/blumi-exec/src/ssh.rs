//! A remote (SSH) execution backend.
//!
//! Commands and file operations run on a remote host over the `ssh` CLI — the
//! same captured-subprocess model as the other backends, no SSH-client crate.
//! Paths are remote-absolute (the engine points the agent's working dir at the
//! remote workspace), so `read_file`/`write_file` go through `cat`/`tee` on the
//! far side.

use async_trait::async_trait;
use blumi_core::{DirEntry, ExecError, ExecOutput, ExecRequest, Executor};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const FILE_OP_TIMEOUT: Duration = Duration::from_secs(30);

enum Outcome {
    Cancelled,
    Finished(Option<std::process::ExitStatus>, bool),
}

/// Runs commands + file ops on a remote host via `ssh`.
pub struct SshExecutor {
    host: String,
    remote_workdir: PathBuf,
    opts: Vec<String>,
}

impl SshExecutor {
    /// `host` is an ssh destination (`user@host` or a configured alias);
    /// `remote_workdir` is an absolute path on the remote.
    pub fn new(host: impl Into<String>, remote_workdir: impl Into<String>) -> Self {
        let rw = remote_workdir.into();
        let remote_workdir = if rw.trim().is_empty() { ".".into() } else { rw };
        SshExecutor {
            host: host.into(),
            remote_workdir: PathBuf::from(remote_workdir),
            opts: vec![
                "-o".into(),
                "BatchMode=yes".into(),
                "-o".into(),
                "ConnectTimeout=10".into(),
                "-o".into(),
                "StrictHostKeyChecking=accept-new".into(),
            ],
        }
    }

    /// Run a remote command string, optionally feeding `stdin`; returns raw
    /// (stdout, stderr, exit_code, timed_out).
    async fn ssh_raw(
        &self,
        remote_cmd: &str,
        stdin: Option<&[u8]>,
        timeout: Option<Duration>,
        ct: &CancellationToken,
    ) -> Result<(Vec<u8>, Vec<u8>, i32, bool), ExecError> {
        let mut cmd = Command::new("ssh");
        cmd.args(&self.opts)
            .arg(&self.host)
            .arg(remote_cmd)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(ExecError::from)?;

        // Spawn the output readers first so a chatty remote can't deadlock while
        // we're still writing stdin.
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
        if let Some(data) = stdin {
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(data).await;
                let _ = si.shutdown().await;
            }
        }

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
                    let (status, timed_out) = r.map_err(ExecError::from)?;
                    Outcome::Finished(status, timed_out)
                }
            }
        };
        let (exit_code, timed_out) = match outcome {
            Outcome::Cancelled => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(ExecError::Cancelled);
            }
            Outcome::Finished(Some(s), false) => (s.code().unwrap_or(-1), false),
            Outcome::Finished(_, t) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                (-1, t)
            }
        };
        let stdout = out_task.await.unwrap_or_default();
        let stderr = err_task.await.unwrap_or_default();
        Ok((stdout, stderr, exit_code, timed_out))
    }
}

#[async_trait]
impl Executor for SshExecutor {
    async fn exec(&self, req: ExecRequest, ct: CancellationToken) -> Result<ExecOutput, ExecError> {
        let cwd = req
            .cwd
            .clone()
            .unwrap_or_else(|| self.remote_workdir.to_string_lossy().into_owned());
        let mut prelude = String::new();
        for (k, v) in &req.env {
            prelude.push_str(&format!("export {k}={}; ", shq(v)));
        }
        let remote = format!("cd {} && {prelude}{}", shq(&cwd), req.command);
        let (stdout, stderr, exit_code, timed_out) =
            self.ssh_raw(&remote, None, req.timeout, &ct).await?;
        Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code,
            timed_out,
        })
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ExecError> {
        let p = path.to_string_lossy();
        let (stdout, stderr, code, _) = self
            .ssh_raw(
                &format!("cat -- {}", shq(&p)),
                None,
                Some(FILE_OP_TIMEOUT),
                &CancellationToken::new(),
            )
            .await?;
        if code != 0 {
            return Err(ExecError::Io(format!(
                "remote read {p} failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        Ok(stdout)
    }

    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<(), ExecError> {
        let p = path.to_string_lossy();
        let dir = path
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        let cmd = format!("mkdir -p {} && cat > {}", shq(&dir), shq(&p));
        let (_, stderr, code, _) = self
            .ssh_raw(
                &cmd,
                Some(contents),
                Some(FILE_OP_TIMEOUT),
                &CancellationToken::new(),
            )
            .await?;
        if code != 0 {
            return Err(ExecError::Io(format!(
                "remote write {p} failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ExecError> {
        let p = path.to_string_lossy();
        let (_, _, code, _) = self
            .ssh_raw(
                &format!("rm -f -- {}", shq(&p)),
                None,
                Some(FILE_OP_TIMEOUT),
                &CancellationToken::new(),
            )
            .await?;
        if code != 0 {
            return Err(ExecError::Io(format!("remote rm {p} failed")));
        }
        Ok(())
    }

    async fn exists(&self, path: &Path) -> Result<bool, ExecError> {
        let p = path.to_string_lossy();
        let (_, _, code, _) = self
            .ssh_raw(
                &format!("test -e {}", shq(&p)),
                None,
                Some(FILE_OP_TIMEOUT),
                &CancellationToken::new(),
            )
            .await?;
        Ok(code == 0)
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, ExecError> {
        let p = path.to_string_lossy();
        let (stdout, stderr, code, _) = self
            .ssh_raw(
                &format!("ls -1Ap -- {}", shq(&p)),
                None,
                Some(FILE_OP_TIMEOUT),
                &CancellationToken::new(),
            )
            .await?;
        if code != 0 {
            return Err(ExecError::Io(format!(
                "remote ls {p} failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        let mut entries = Vec::new();
        for line in String::from_utf8_lossy(&stdout).lines() {
            let line = line.trim_end_matches('\n');
            if line.is_empty() {
                continue;
            }
            let is_dir = line.ends_with('/');
            let name = line.trim_end_matches('/').to_string();
            if name.is_empty() {
                continue;
            }
            entries.push(DirEntry {
                name,
                is_dir,
                size: 0,
            });
        }
        entries.sort_by(|a, b| (!a.is_dir, &a.name).cmp(&(!b.is_dir, &b.name)));
        Ok(entries)
    }

    fn working_dir(&self) -> &Path {
        &self.remote_workdir
    }
}

/// POSIX single-quote a string for safe use in a remote shell command.
fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quotes_safely() {
        assert_eq!(shq("plain"), "'plain'");
        assert_eq!(shq("a b"), "'a b'");
        assert_eq!(shq("it's"), "'it'\\''s'");
        assert_eq!(shq("$(rm -rf /)"), "'$(rm -rf /)'"); // inert inside quotes
    }

    #[test]
    fn defaults_remote_workdir() {
        let e = SshExecutor::new("user@host", "");
        assert_eq!(e.working_dir(), Path::new("."));
        let e = SshExecutor::new("host", "/srv/app");
        assert_eq!(e.working_dir(), Path::new("/srv/app"));
        assert!(e.opts.iter().any(|o| o == "BatchMode=yes"));
    }

    #[tokio::test]
    #[ignore = "requires an ssh host (set BLUMI_TEST_SSH_HOST)"]
    async fn ssh_roundtrip() {
        let host = std::env::var("BLUMI_TEST_SSH_HOST").unwrap_or_else(|_| "localhost".into());
        let dir = std::env::var("BLUMI_TEST_SSH_DIR").unwrap_or_else(|_| "/tmp".into());
        let e = SshExecutor::new(host, &dir);
        let out = e
            .exec(ExecRequest::new("echo remote-ok"), CancellationToken::new())
            .await
            .unwrap();
        assert!(out.stdout.contains("remote-ok"), "stderr: {}", out.stderr);

        let f = format!("{dir}/blumi_ssh_test.txt");
        e.write_file(Path::new(&f), b"hello ssh").await.unwrap();
        assert!(e.exists(Path::new(&f)).await.unwrap());
        let back = e.read_file(Path::new(&f)).await.unwrap();
        assert_eq!(&back, b"hello ssh");
        e.remove_file(Path::new(&f)).await.unwrap();
        assert!(!e.exists(Path::new(&f)).await.unwrap());
    }
}
