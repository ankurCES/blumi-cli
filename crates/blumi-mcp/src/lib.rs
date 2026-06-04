//! MCP (Model Context Protocol) client integration.
//!
//! Connects to external MCP servers over stdio via [`rmcp`], discovers their
//! tools, and adapts each as a [`blumi_core::Tool`] named `mcp__<server>__<tool>`
//! that proxies to the server. The running client is kept alive by the tools
//! (each holds an `Arc` to it).

use async_trait::async_trait;
use blumi_core::{Tool, ToolContext, ToolError};
use blumi_protocol::ToolResult;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// A running MCP client (owns the server child process).
type McpClient = RunningService<RoleClient, ()>;

/// Connect to one MCP server over stdio, returning its tools adapted as blumi
/// tools. The returned tools keep the connection (and child process) alive.
pub async fn connect_server(
    name: &str,
    command: &str,
    args: &[String],
    env: &[(String, String)],
) -> anyhow::Result<Vec<Arc<dyn Tool>>> {
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }

    // Capture the server's stderr instead of letting it inherit our terminal —
    // MCP servers log human-readable notices there (e.g. "client does not support
    // MCP roots…", "not a valid git repo") which would otherwise corrupt the TUI's
    // alternate screen. We drain it into `tracing` (a log file under the TUI), so
    // it's available for debugging but never painted over the UI.
    let (transport, stderr) = TokioChildProcess::builder(cmd)
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stderr) = stderr {
        let server = name.to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if !line.is_empty() {
                    tracing::debug!(target: "mcp::server", server = %server, "{line}");
                }
            }
        });
    }
    let client: Arc<McpClient> = Arc::new(().serve(transport).await?);

    let listed = client.list_tools(Default::default()).await?;
    let tools: Vec<Arc<dyn Tool>> = listed
        .tools
        .into_iter()
        .map(|t| Arc::new(McpTool::new(name, client.clone(), t)) as Arc<dyn Tool>)
        .collect();

    tracing::info!(
        server = name,
        count = tools.len(),
        "connected to MCP server"
    );
    Ok(tools)
}

/// A single MCP tool exposed by a server, proxied to via the shared client.
struct McpTool {
    full_name: String,
    tool_name: String,
    description: String,
    schema: serde_json::Value,
    client: Arc<McpClient>,
}

impl McpTool {
    fn new(server: &str, client: Arc<McpClient>, tool: rmcp::model::Tool) -> Self {
        let tool_name = tool.name.to_string();
        let full_name = format!("mcp__{server}__{tool_name}");
        let description = tool.description.map(|d| d.to_string()).unwrap_or_default();
        let schema = serde_json::to_value(&tool.input_schema)
            .unwrap_or_else(|_| serde_json::json!({ "type": "object" }));
        McpTool {
            full_name,
            tool_name,
            description,
            schema,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.full_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
        ct: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let mut params = CallToolRequestParams::new(self.tool_name.clone());
        if let Some(map) = input.as_object().cloned() {
            params = params.with_arguments(map);
        }

        let result = tokio::select! {
            _ = ct.cancelled() => return Err(ToolError::Cancelled),
            r = self.client.call_tool(params) => match r {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("MCP call failed: {e}"))),
            },
        };

        // Extract text content + error flag from the serialized result (robust
        // across rmcp's exact field accessors).
        let value = serde_json::to_value(&result).unwrap_or_default();
        let text = value
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let is_error = value
            .get("isError")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let text = if text.is_empty() {
            "(no text content)".to_string()
        } else {
            text
        };

        if is_error {
            Ok(ToolResult::error(text))
        } else {
            Ok(ToolResult::success(text))
        }
    }
}
