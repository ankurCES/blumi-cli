//! A minimal JSON-RPC-over-stdio LSP client: spawn a server, perform the
//! initialize handshake, and issue requests/notifications with id correlation.

use crate::framing::{encode, read_message};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

/// A running language server connection.
pub struct LspClient {
    // Held so `kill_on_drop` tears the server down with the client.
    #[allow(dead_code)]
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    next_id: AtomicI64,
    pending: Pending,
}

impl LspClient {
    /// Spawn `command args` and run the `initialize` → `initialized` handshake
    /// rooted at `root_uri`.
    pub async fn start(
        command: &str,
        args: &[String],
        root_uri: &str,
    ) -> anyhow::Result<Arc<Self>> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("language server has no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("language server has no stdout"))?;
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        // Reader task: route responses to their waiter by id; ignore
        // server-initiated requests/notifications.
        {
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut r = BufReader::new(stdout);
                while let Ok(Some(msg)) = read_message(&mut r).await {
                    let is_response = msg.get("result").is_some() || msg.get("error").is_some();
                    if is_response {
                        if let Some(id) = msg.get("id").and_then(Value::as_i64) {
                            if let Some(tx) = pending.lock().await.remove(&id) {
                                let _ = tx.send(msg);
                            }
                        }
                    }
                }
            });
        }

        let client = Arc::new(LspClient {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            next_id: AtomicI64::new(1),
            pending,
        });

        let init = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "linkSupport": true },
                    "references": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
                }
            },
            "clientInfo": { "name": "blumi", "version": env!("CARGO_PKG_VERSION") }
        });
        client
            .request("initialize", init, Duration::from_secs(20))
            .await?;
        client.notify("initialized", json!({})).await?;
        Ok(client)
    }

    /// Send a request and await its result (or error).
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => {
                if let Some(err) = resp.get("error") {
                    anyhow::bail!("language server error: {err}");
                }
                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
            Ok(Err(_)) => anyhow::bail!("language server closed the connection"),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                anyhow::bail!("language server request '{method}' timed out");
            }
        }
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn write(&self, msg: &Value) -> anyhow::Result<()> {
        let bytes = encode(msg);
        let mut w = self.stdin.lock().await;
        w.write_all(&bytes).await?;
        w.flush().await?;
        Ok(())
    }
}
