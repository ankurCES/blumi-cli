//! `run()`-level tests for the self-evolution tools: action dispatch, the
//! validate-before-write guard, and the reload event — driven through a real
//! [`ToolContext`]. (The tools use `std::fs` directly, so the executor is a
//! never-touched stub.)
#![cfg(test)]

use crate::{ReloadTool, SelfConfig, SkillCatalog, SkillManager};
use blumi_core::{
    DirEntry, EventEmitter, ExecError, ExecOutput, ExecRequest, Executor, Interactor, ToolContext,
    TypedTool,
};
use blumi_protocol::{Event, SessionId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct StubExec {
    dir: PathBuf,
}

#[async_trait::async_trait]
impl Executor for StubExec {
    async fn exec(
        &self,
        _req: ExecRequest,
        _ct: CancellationToken,
    ) -> Result<ExecOutput, ExecError> {
        Err(ExecError::Unavailable("stub".into()))
    }
    async fn read_file(&self, _path: &Path) -> Result<Vec<u8>, ExecError> {
        Err(ExecError::Unavailable("stub".into()))
    }
    async fn write_file(&self, _path: &Path, _contents: &[u8]) -> Result<(), ExecError> {
        Err(ExecError::Unavailable("stub".into()))
    }
    async fn exists(&self, _path: &Path) -> Result<bool, ExecError> {
        Ok(false)
    }
    async fn list_dir(&self, _path: &Path) -> Result<Vec<DirEntry>, ExecError> {
        Ok(vec![])
    }
    fn working_dir(&self) -> &Path {
        &self.dir
    }
}

fn ctx(dir: &Path) -> (ToolContext, mpsc::UnboundedReceiver<Event>) {
    let (etx, erx) = mpsc::unbounded_channel();
    let (itx, _irx) = mpsc::unbounded_channel();
    let cx = ToolContext {
        session_id: SessionId::from("test"),
        working_dir: dir.to_path_buf(),
        executor: Arc::new(StubExec {
            dir: dir.to_path_buf(),
        }),
        events: EventEmitter::new(etx),
        interactor: Interactor::new(itx),
        spawner: None,
        journal: None,
    };
    (cx, erx)
}

fn input<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn manage_skill_create_delete_and_reject() {
    let dir = tempfile::tempdir().unwrap();
    let (cx, _rx) = ctx(dir.path());
    let tool = SkillManager::new(dir.path().to_path_buf());

    // create → discoverable
    let res = tool
        .run(
            input(serde_json::json!({
                "action": "create", "name": "pdf-wrangler",
                "description": "Work with PDFs", "instructions": "Use pdftotext."
            })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!res.is_error(), "{}", res.model_preview);
    assert!(SkillCatalog::load(&[dir.path().to_path_buf()])
        .get("pdf-wrangler")
        .is_some());

    // path-escape slug → rejected, nothing written
    let res = tool
        .run(
            input(serde_json::json!({
                "action": "create", "name": "../escape",
                "description": "x", "instructions": "y"
            })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(res.is_error());

    // delete → gone
    let res = tool
        .run(
            input(serde_json::json!({ "action": "delete", "name": "pdf-wrangler" })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!res.is_error(), "{}", res.model_preview);
    assert!(SkillCatalog::load(&[dir.path().to_path_buf()])
        .get("pdf-wrangler")
        .is_none());
}

#[tokio::test]
async fn self_config_set_validates_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let (cx, _rx) = ctx(dir.path());
    let tool = SelfConfig::new(settings.clone());

    // valid set → written
    let res = tool
        .run(
            input(serde_json::json!({
                "action": "set", "key": "llm.temperature", "value": "0.3"
            })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!res.is_error(), "{}", res.model_preview);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(saved["llm"]["temperature"], serde_json::json!(0.3));

    // invalid type (personas must be a map) → rejected, file unchanged
    let before = std::fs::read_to_string(&settings).unwrap();
    let res = tool
        .run(
            input(serde_json::json!({
                "action": "set", "key": "personas", "value": "5"
            })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(res.is_error());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
}

#[tokio::test]
async fn self_config_add_persona() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let (cx, _rx) = ctx(dir.path());
    let tool = SelfConfig::new(settings.clone());

    let res = tool
        .run(
            input(serde_json::json!({
                "action": "add_persona", "name": "pirate",
                "description": "Arr", "instructions": "Talk like a pirate."
            })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!res.is_error(), "{}", res.model_preview);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(saved["personas"]["pirate"]["description"], "Arr");
}

#[tokio::test]
async fn reload_self_emits_reload_event() {
    let dir = tempfile::tempdir().unwrap();
    let (cx, mut rx) = ctx(dir.path());
    let tool = ReloadTool::new();

    let res = tool
        .run(
            input(serde_json::json!({ "reason": "added the pirate persona" })),
            &cx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(res.break_turn);

    let ev = rx.try_recv().expect("a Reload event was emitted");
    match ev {
        Event::Reload { reason } => assert_eq!(reason, "added the pirate persona"),
        other => panic!("expected Reload, got {other:?}"),
    }
}
