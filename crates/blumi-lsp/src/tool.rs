//! `Lsp`: a generic code-intelligence tool backed by configured language
//! servers — definitions, references, hover, and document symbols.

use crate::client::LspClient;
use async_trait::async_trait;
use blumi_core::{ToolContext, ToolError, TypedTool};
use blumi_protocol::ToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// A configured language server.
#[derive(Debug, Clone)]
pub struct LspServer {
    pub command: String,
    pub args: Vec<String>,
    /// File extensions (without the dot) this server handles, e.g. `["rs"]`.
    pub extensions: Vec<String>,
    /// LSP languageId, e.g. `"rust"`.
    pub language_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct LspInput {
    /// One of: `definition`, `references`, `hover`, `symbols`.
    pub action: String,
    /// File path (relative to the project root or absolute).
    pub path: String,
    /// 1-based line (required for definition/references/hover).
    #[serde(default)]
    pub line: Option<u32>,
    /// 1-based column (defaults to 1).
    #[serde(default)]
    pub character: Option<u32>,
}

/// Spawns + reuses language servers and answers code-intel queries.
pub struct LspTool {
    servers: Vec<LspServer>,
    root_dir: PathBuf,
    root_uri: String,
    clients: Mutex<HashMap<String, Arc<LspClient>>>,
    opened: Mutex<HashSet<String>>,
}

impl LspTool {
    pub fn new(servers: Vec<LspServer>, root_dir: impl Into<PathBuf>) -> Self {
        let root_dir = root_dir.into();
        let root_uri = format!("file://{}", root_dir.display());
        LspTool {
            servers,
            root_dir,
            root_uri,
            clients: Mutex::new(HashMap::new()),
            opened: Mutex::new(HashSet::new()),
        }
    }

    fn server_for(&self, ext: &str) -> Option<&LspServer> {
        self.servers
            .iter()
            .find(|s| s.extensions.iter().any(|e| e == ext))
    }

    async fn client_for(&self, server: &LspServer) -> Result<Arc<LspClient>, ToolError> {
        let mut clients = self.clients.lock().await;
        if let Some(c) = clients.get(&server.command) {
            return Ok(c.clone());
        }
        let client = LspClient::start(&server.command, &server.args, &self.root_uri)
            .await
            .map_err(|e| {
                ToolError::Execution(format!("failed to start {}: {e}", server.command))
            })?;
        clients.insert(server.command.clone(), client.clone());
        Ok(client)
    }

    /// Open `uri` (once) so the server has the document.
    async fn ensure_open(
        &self,
        client: &LspClient,
        uri: &str,
        language_id: &str,
        path: &std::path::Path,
    ) -> Result<(), ToolError> {
        if self.opened.lock().await.contains(uri) {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| ToolError::Execution(format!("could not read {}: {e}", path.display())))?;
        client
            .notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text } }),
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        self.opened.lock().await.insert(uri.to_string());
        Ok(())
    }
}

#[async_trait]
impl TypedTool for LspTool {
    type Input = LspInput;

    fn name(&self) -> &str {
        "Lsp"
    }
    fn description(&self) -> &str {
        "Code intelligence via a language server: action=definition|references|hover|symbols at \
         a file path + 1-based line/character. Use it to jump to definitions, find usages, read \
         types/docs, or outline a file."
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn run(
        &self,
        input: Self::Input,
        _ctx: &ToolContext,
        _ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let path = if std::path::Path::new(&input.path).is_absolute() {
            PathBuf::from(&input.path)
        } else {
            self.root_dir.join(&input.path)
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let Some(server) = self.server_for(&ext).cloned() else {
            return Ok(ToolResult::invalid_input(
                format!("no language server configured for .{ext}"),
                "configure one under [lsp_servers] in settings.json",
            ));
        };

        let client = self.client_for(&server).await?;
        let uri = format!("file://{}", path.display());
        self.ensure_open(&client, &uri, &server.language_id, &path)
            .await?;

        let line = input.line.unwrap_or(1).saturating_sub(1);
        let character = input.character.unwrap_or(1).saturating_sub(1);
        let pos = json!({ "line": line, "character": character });
        let doc = json!({ "uri": uri });
        let to = Duration::from_secs(15);

        let preview = match input.action.as_str() {
            "definition" => {
                let r = client
                    .request(
                        "textDocument/definition",
                        json!({ "textDocument": doc, "position": pos }),
                        to,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                fmt_locations(&r)
            }
            "references" => {
                let r = client
                    .request(
                        "textDocument/references",
                        json!({ "textDocument": doc, "position": pos, "context": { "includeDeclaration": true } }),
                        to,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                fmt_locations(&r)
            }
            "hover" => {
                let r = client
                    .request(
                        "textDocument/hover",
                        json!({ "textDocument": doc, "position": pos }),
                        to,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                fmt_hover(&r)
            }
            "symbols" | "documentSymbol" => {
                let r = client
                    .request(
                        "textDocument/documentSymbol",
                        json!({ "textDocument": doc }),
                        to,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                fmt_symbols(&r)
            }
            other => {
                return Ok(ToolResult::invalid_input(
                    format!("unknown action '{other}'"),
                    "use definition, references, hover, or symbols",
                ))
            }
        };
        Ok(ToolResult::success(preview))
    }
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

/// Format a `Location | Location[] | LocationLink[]` result as `path:line:col`.
fn fmt_locations(result: &Value) -> String {
    let items = match result {
        Value::Array(a) => a.clone(),
        Value::Null => vec![],
        other => vec![other.clone()],
    };
    if items.is_empty() {
        return "no results".into();
    }
    let mut out = String::new();
    for loc in &items {
        let uri = loc
            .get("uri")
            .or_else(|| loc.get("targetUri"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let start = loc
            .get("range")
            .or_else(|| loc.get("targetRange"))
            .and_then(|r| r.get("start"));
        let line = start
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let ch = start
            .and_then(|s| s.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        out.push_str(&format!("{}:{}:{}\n", uri_to_path(uri), line + 1, ch + 1));
    }
    out.trim_end().to_string()
}

/// Format a hover result's contents (MarkupContent / string / MarkedString[]).
fn fmt_hover(result: &Value) -> String {
    fn one(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => o
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        }
    }
    match result.get("contents") {
        Some(Value::Array(a)) => a
            .iter()
            .map(one)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        Some(v) => {
            let s = one(v);
            if s.trim().is_empty() {
                "no hover info".into()
            } else {
                s.trim().to_string()
            }
        }
        None => "no hover info".into(),
    }
}

/// Format `DocumentSymbol[]` (hierarchical) or `SymbolInformation[]` (flat).
fn fmt_symbols(result: &Value) -> String {
    let arr = result.as_array().cloned().unwrap_or_default();
    if arr.is_empty() {
        return "no symbols".into();
    }
    fn walk(sym: &Value, depth: usize, out: &mut String) {
        let name = sym.get("name").and_then(Value::as_str).unwrap_or("?");
        let kind = sym
            .get("kind")
            .and_then(Value::as_u64)
            .map(symbol_kind)
            .unwrap_or("symbol");
        out.push_str(&format!("{}{kind} {name}\n", "  ".repeat(depth)));
        if let Some(children) = sym.get("children").and_then(Value::as_array) {
            for c in children {
                walk(c, depth + 1, out);
            }
        }
    }
    let mut out = String::new();
    for s in &arr {
        walk(s, 0, &mut out);
    }
    out.trim_end().to_string()
}

/// LSP SymbolKind → a short label.
fn symbol_kind(k: u64) -> &'static str {
    match k {
        2 => "module",
        5 => "class",
        6 => "method",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        23 => "struct",
        26 => "type",
        _ => "symbol",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_locations_variants() {
        let single =
            json!({ "uri": "file:///p/a.rs", "range": { "start": { "line": 9, "character": 4 } } });
        assert_eq!(fmt_locations(&single), "/p/a.rs:10:5");

        let arr = json!([
            { "uri": "file:///p/a.rs", "range": { "start": { "line": 0, "character": 0 } } },
            { "targetUri": "file:///p/b.rs", "targetRange": { "start": { "line": 4, "character": 2 } } }
        ]);
        assert_eq!(fmt_locations(&arr), "/p/a.rs:1:1\n/p/b.rs:5:3");

        assert_eq!(fmt_locations(&Value::Null), "no results");
    }

    #[test]
    fn formats_hover() {
        let markup = json!({ "contents": { "kind": "markdown", "value": "fn foo() -> i32" } });
        assert_eq!(fmt_hover(&markup), "fn foo() -> i32");
        let arr = json!({ "contents": ["a", { "value": "b" }] });
        assert_eq!(fmt_hover(&arr), "a\nb");
        assert_eq!(fmt_hover(&json!({})), "no hover info");
    }

    #[test]
    fn formats_symbols_hierarchically() {
        let syms = json!([
            { "name": "Foo", "kind": 23, "children": [
                { "name": "bar", "kind": 6 }
            ] },
            { "name": "main", "kind": 12 }
        ]);
        assert_eq!(
            fmt_symbols(&syms),
            "struct Foo\n  method bar\nfunction main"
        );
        assert_eq!(fmt_symbols(&json!([])), "no symbols");
    }
}
