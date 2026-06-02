//! A Docker sandbox execution backend.
//!
//! Commands (`Bash`) run inside a long-lived container, so destructive or
//! untrusted commands hit the sandbox rather than the host. The project
//! directory is bind-mounted at [`CONTAINER_WORKDIR`], so file tools operate on
//! the shared workspace (delegated to a [`LocalExecutor`]) and the container
//! sees the same files.
//!
//! Implemented by driving the `docker` CLI — no daemon-client dependency, and
//! it reuses the same captured-subprocess model as the local backend.

use crate::LocalExecutor;
use async_trait::async_trait;
use blumi_core::{DirEntry, ExecError, ExecOutput, ExecRequest, Executor};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// Where the host working directory is bind-mounted inside the container.
pub const CONTAINER_WORKDIR: &str = "/workspace";

enum Outcome {
    Cancelled,
    /// (exit status if completed, timed-out flag)
    Finished(Option<std::process::ExitStatus>, bool),
}

/// Runs commands inside a Docker container; file ops use the bind-mounted host
/// workspace via an inner [`LocalExecutor`].
pub struct DockerExecutor {
    container: String,
    host_workdir: PathBuf,
    local: LocalExecutor,
}

impl DockerExecutor {
    /// Start a detached container from `image` with `working_dir` bind-mounted,
    /// ready to `exec` into. Pulls the image on first use (via `docker run`).
    pub async fn start(image: &str, working_dir: impl Into<PathBuf>) -> Result<Self, ExecError> {
        let host = working_dir.into();
        let bind = format!("{}:{}", host.display(), CONTAINER_WORKDIR);
        let args = [
            "run",
            "-d",
            "-v",
            &bind,
            "-w",
            CONTAINER_WORKDIR,
            image,
            "sleep",
            "infinity",
        ]
        .map(String::from);

        let out = run_docker(
            &args,
            Some(Duration::from_secs(180)),
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| ExecError::Unavailable(format!("could not start docker container: {e}")))?;
        if out.exit_code != 0 {
            return Err(ExecError::Unavailable(format!(
                "docker run failed: {}",
                out.stderr.trim()
            )));
        }
        let container = out.stdout.trim().to_string();
        if container.is_empty() {
            return Err(ExecError::Unavailable(
                "docker run returned no container id".into(),
            ));
        }
        Ok(DockerExecutor {
            container,
            host_workdir: host.clone(),
            local: LocalExecutor::new(host),
        })
    }

    /// Map a host path to the corresponding path inside the container.
    fn container_path(&self, host: Option<&str>) -> String {
        match host {
            Some(c) => match Path::new(c).strip_prefix(&self.host_workdir) {
                Ok(rel) if rel.as_os_str().is_empty() => CONTAINER_WORKDIR.to_string(),
                Ok(rel) => format!("{CONTAINER_WORKDIR}/{}", rel.display()),
                Err(_) => CONTAINER_WORKDIR.to_string(),
            },
            None => CONTAINER_WORKDIR.to_string(),
        }
    }
}

#[async_trait]
impl Executor for DockerExecutor {
    async fn exec(&self, req: ExecRequest, ct: CancellationToken) -> Result<ExecOutput, ExecError> {
        let cwd = self.container_path(req.cwd.as_deref());
        let mut args = vec!["exec".to_string(), "-w".to_string(), cwd];
        for (k, v) in &req.env {
            args.push("-e".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push(self.container.clone());
        args.push("/bin/sh".to_string());
        args.push("-c".to_string());
        args.push(req.command.clone());
        run_docker(&args, req.timeout, &ct).await
    }

    // File operations target the bind-mounted host workspace.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ExecError> {
        self.local.read_file(path).await
    }
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<(), ExecError> {
        self.local.write_file(path, contents).await
    }
    async fn remove_file(&self, path: &Path) -> Result<(), ExecError> {
        self.local.remove_file(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool, ExecError> {
        self.local.exists(path).await
    }
    async fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, ExecError> {
        self.local.list_dir(path).await
    }
    fn working_dir(&self) -> &Path {
        self.local.working_dir()
    }

    async fn cleanup(&self) -> Result<(), ExecError> {
        let args = ["rm", "-f", &self.container].map(String::from);
        let _ = run_docker(
            &args,
            Some(Duration::from_secs(30)),
            &CancellationToken::new(),
        )
        .await;
        Ok(())
    }
}

impl Drop for DockerExecutor {
    fn drop(&mut self) {
        // Best-effort: remove the container so it doesn't leak on exit.
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.container])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// Run `docker <args>`, capturing output, honoring a timeout and cancellation.
async fn run_docker(
    args: &[String],
    timeout: Option<Duration>,
    ct: &CancellationToken,
) -> Result<ExecOutput, ExecError> {
    let mut cmd = Command::new("docker");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(ExecError::from)?;

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

    // Scope `wait` so its mutable borrow of `child` ends before we may kill it.
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
        Outcome::Finished(Some(status), false) => (status.code().unwrap_or(-1), false),
        Outcome::Finished(_, timed_out) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            (-1, timed_out)
        }
    };

    let stdout = String::from_utf8_lossy(&out_task.await.unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_task.await.unwrap_or_default()).into_owned();
    Ok(ExecOutput {
        stdout,
        stderr,
        exit_code,
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a running docker daemon"]
    async fn docker_exec_runs_in_container() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hi from host").unwrap();

        let exec = DockerExecutor::start("alpine:latest", dir.path())
            .await
            .expect("start container");

        // The command runs inside the container, seeing the bind-mounted file.
        let out = exec
            .exec(
                ExecRequest::new("echo from-container && cat /workspace/hello.txt"),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.success(), "stderr: {}", out.stderr);
        assert!(out.stdout.contains("from-container"));
        assert!(out.stdout.contains("hi from host"));

        // File ops go through the host bind mount and are visible in-container.
        exec.write_file(&dir.path().join("new.txt"), b"made on host")
            .await
            .unwrap();
        let seen = exec
            .exec(
                ExecRequest::new("cat /workspace/new.txt"),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(seen.stdout.contains("made on host"));

        exec.cleanup().await.unwrap();
    }

    #[test]
    fn maps_container_paths() {
        // Build without starting a container.
        let de = DockerExecutor {
            container: "x".into(),
            host_workdir: PathBuf::from("/work/proj"),
            local: LocalExecutor::new("/work/proj"),
        };
        assert_eq!(de.container_path(Some("/work/proj")), "/workspace");
        assert_eq!(de.container_path(Some("/work/proj/src")), "/workspace/src");
        assert_eq!(de.container_path(Some("/elsewhere")), "/workspace");
        assert_eq!(de.container_path(None), "/workspace");
    }
}
