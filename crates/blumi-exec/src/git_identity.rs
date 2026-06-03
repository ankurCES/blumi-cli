//! An executor wrapper that stamps a default git identity onto every command.
//!
//! Git reads `GIT_AUTHOR_*` / `GIT_COMMITTER_*` from the environment in
//! preference to `user.name`/`user.email` config, so injecting them into each
//! command the agent runs makes any `git commit` (and gh flows that commit via
//! git) authored consistently — regardless of the repo's or host's git config,
//! and without the agent having to remember a `--author` flag.

use async_trait::async_trait;
use blumi_core::{DirEntry, ExecError, ExecOutput, ExecRequest, Executor};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Wraps any [`Executor`], injecting a fixed git author + committer identity
/// into every command's environment. Per-command env that already sets one of
/// these wins (we only fill in what's missing).
pub struct GitIdentityExecutor {
    inner: Arc<dyn Executor>,
    name: String,
    email: String,
}

impl GitIdentityExecutor {
    pub fn new(
        inner: Arc<dyn Executor>,
        name: impl Into<String>,
        email: impl Into<String>,
    ) -> Self {
        GitIdentityExecutor {
            inner,
            name: name.into(),
            email: email.into(),
        }
    }
}

#[async_trait]
impl Executor for GitIdentityExecutor {
    async fn init_session(&self) -> Result<(), ExecError> {
        self.inner.init_session().await
    }

    async fn exec(
        &self,
        mut req: ExecRequest,
        ct: CancellationToken,
    ) -> Result<ExecOutput, ExecError> {
        for (k, v) in [
            ("GIT_AUTHOR_NAME", self.name.as_str()),
            ("GIT_COMMITTER_NAME", self.name.as_str()),
            ("GIT_AUTHOR_EMAIL", self.email.as_str()),
            ("GIT_COMMITTER_EMAIL", self.email.as_str()),
            ("EMAIL", self.email.as_str()),
        ] {
            req.env
                .entry(k.to_string())
                .or_insert_with(|| v.to_string());
        }
        self.inner.exec(req, ct).await
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ExecError> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<(), ExecError> {
        self.inner.write_file(path, contents).await
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ExecError> {
        self.inner.remove_file(path).await
    }

    async fn exists(&self, path: &Path) -> Result<bool, ExecError> {
        self.inner.exists(path).await
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, ExecError> {
        self.inner.list_dir(path).await
    }

    async fn cleanup(&self) -> Result<(), ExecError> {
        self.inner.cleanup().await
    }

    fn working_dir(&self) -> &Path {
        self.inner.working_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Captures the env it was exec'd with.
    struct Spy(Mutex<std::collections::BTreeMap<String, String>>);
    #[async_trait]
    impl Executor for Spy {
        async fn exec(
            &self,
            req: ExecRequest,
            _ct: CancellationToken,
        ) -> Result<ExecOutput, ExecError> {
            *self.0.lock().unwrap() = req.env.clone();
            Ok(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
            })
        }
        async fn read_file(&self, _p: &Path) -> Result<Vec<u8>, ExecError> {
            Ok(vec![])
        }
        async fn write_file(&self, _p: &Path, _c: &[u8]) -> Result<(), ExecError> {
            Ok(())
        }
        async fn exists(&self, _p: &Path) -> Result<bool, ExecError> {
            Ok(false)
        }
        async fn list_dir(&self, _p: &Path) -> Result<Vec<DirEntry>, ExecError> {
            Ok(vec![])
        }
        fn working_dir(&self) -> &Path {
            Path::new(".")
        }
    }

    #[tokio::test]
    async fn stamps_git_identity_into_env() {
        let spy = Arc::new(Spy(Mutex::new(Default::default())));
        let exec = GitIdentityExecutor::new(spy.clone(), "Blumi", "ankur.nairit@gmail.com");
        exec.exec(
            ExecRequest::new("git commit -m x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let env = spy.0.lock().unwrap().clone();
        assert_eq!(env.get("GIT_AUTHOR_NAME").unwrap(), "Blumi");
        assert_eq!(env.get("GIT_COMMITTER_NAME").unwrap(), "Blumi");
        assert_eq!(
            env.get("GIT_AUTHOR_EMAIL").unwrap(),
            "ankur.nairit@gmail.com"
        );
        assert_eq!(
            env.get("GIT_COMMITTER_EMAIL").unwrap(),
            "ankur.nairit@gmail.com"
        );
    }

    #[tokio::test]
    async fn does_not_override_explicit_env() {
        let spy = Arc::new(Spy(Mutex::new(Default::default())));
        let exec = GitIdentityExecutor::new(spy.clone(), "Blumi", "blumi@example.com");
        let mut req = ExecRequest::new("git commit");
        req.env.insert("GIT_AUTHOR_NAME".into(), "Someone".into());
        exec.exec(req, CancellationToken::new()).await.unwrap();
        let env = spy.0.lock().unwrap().clone();
        assert_eq!(env.get("GIT_AUTHOR_NAME").unwrap(), "Someone");
        // The unset ones still get the default.
        assert_eq!(env.get("GIT_COMMITTER_NAME").unwrap(), "Blumi");
    }
}
