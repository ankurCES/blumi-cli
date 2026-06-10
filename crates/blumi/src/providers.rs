//! Shared provider/model catalog + settings persistence, used by both the web
//! Control Center and the TUI provider/model pickers.

use blumi_config::BlumiConfig;
use serde_json::{json, Value};
use std::path::Path;

/// A few suggested model ids per known provider (for the pickers).
pub fn suggested_models(provider: &str) -> Vec<String> {
    let m: &[&str] = match provider {
        "anthropic" | "azure-foundry" => &[
            "claude-opus-4-8",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-1",
            "claude-3-5-haiku-latest",
        ],
        "openai" => &["gpt-4o", "gpt-4o-mini", "o4-mini"],
        "gemini" => &["gemini-2.0-flash", "gemini-1.5-pro"],
        "openrouter" => &[
            "anthropic/claude-3.7-sonnet",
            "openai/gpt-4o",
            "google/gemini-2.0-flash-001",
        ],
        "deepseek" => &["deepseek-chat", "deepseek-reasoner"],
        "groq" => &["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
        "minimax" => &["MiniMax-M3", "MiniMax-M2", "MiniMax-Text-01"],
        "ollama" => &["llama3.1", "qwen2.5-coder", "deepseek-r1"],
        "local-mlx" => &[
            "mlx-community/Qwen2.5-Coder-7B-Instruct-4bit",
            "mlx-community/Llama-3.2-3B-Instruct-4bit",
            "mlx-community/bge-small-en-v1.5",
        ],
        "local-cuda" => &["Qwen2.5-Coder-7B-Instruct", "llama3.1", "nomic-embed-text"],
        // Unknown/local provider: no fixed catalog — the user names the model
        // (`/model <id>`). (Keeping this empty is load-bearing: persist_provider
        // uses the first entry as the default model on a provider switch.)
        _ => &[],
    };
    m.iter().map(|s| s.to_string()).collect()
}

/// Fetch the active provider's live model catalog using its configured key
/// (best-effort). Returns chat-capable model ids, or `None` when the provider
/// doesn't support a models listing / the call fails — callers then fall back to
/// [`suggested_models`]. Handles Anthropic (`/v1/models`), Gemini
/// (`/v1beta/models`), and the OpenAI-compatible `{base_url}/models` shape that
/// every other provider (openai, openrouter, deepseek, groq, minimax, ollama,
/// local, …) speaks. Azure Foundry uses deployments, not a catalog, so it stays
/// on the static list.
pub async fn fetch_models(c: &BlumiConfig) -> Option<Vec<String>> {
    let provider = c.llm.provider.clone();
    if provider.is_empty() || provider == "mock" || provider == "azure-foundry" {
        return None;
    }
    let pc = c.providers.get(&provider)?;
    let key = pc.resolve_api_key();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .ok()?;

    let resp = match provider.as_str() {
        "anthropic" => {
            client
                .get("https://api.anthropic.com/v1/models?limit=1000")
                .header("x-api-key", key?)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
        }
        "gemini" => {
            client
                .get(format!(
                    "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000&key={}",
                    key?
                ))
                .send()
                .await
        }
        _ => {
            // OpenAI + every OpenAI-compatible provider: `{base_url}/models`.
            let base = pc.base_url.clone()?;
            let mut req = client.get(format!("{}/models", base.trim_end_matches('/')));
            if let Some(k) = key {
                req = req.bearer_auth(k);
            }
            req.send().await
        }
    };

    let v: Value = resp.ok()?.json().await.ok()?;
    let mut out: Vec<String> = Vec::new();
    // OpenAI / Anthropic shape: `{ "data": [ { "id": "…" } ] }`.
    if let Some(arr) = v.get("data").and_then(Value::as_array) {
        for id in arr
            .iter()
            .filter_map(|it| it.get("id").and_then(Value::as_str))
        {
            out.push(id.to_string());
        }
    } else if let Some(arr) = v.get("models").and_then(Value::as_array) {
        // Gemini shape: `{ "models": [ { "name": "models/…", … } ] }`.
        for it in arr {
            let chat = it
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .map(|m| m.iter().any(|s| s.as_str() == Some("generateContent")))
                .unwrap_or(true);
            if let Some(name) = it.get("name").and_then(Value::as_str) {
                if chat {
                    out.push(name.trim_start_matches("models/").to_string());
                }
            }
        }
    }

    // Drop obvious non-chat models (embeddings / audio / image / moderation) and
    // cap the list so the picker stays manageable; the picker is fuzzy-filterable.
    out.retain(|m| is_chat_model(m));
    out.truncate(60);
    (!out.is_empty()).then_some(out)
}

/// Heuristic: keep chat/completion models, drop embedding/audio/image/etc.
fn is_chat_model(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    ![
        "embed",
        "whisper",
        "tts",
        "dall-e",
        "dalle",
        "moderation",
        "rerank",
        "audio",
    ]
    .iter()
    .any(|bad| l.contains(bad))
}

/// Human label for a provider name.
pub fn provider_label(name: &str) -> String {
    match name {
        "anthropic" => "Anthropic (Claude)",
        "azure-foundry" => "Azure AI Foundry",
        "openai" => "OpenAI",
        "gemini" => "Google Gemini",
        "openrouter" => "OpenRouter",
        "deepseek" => "DeepSeek",
        "minimax" => "MiniMax",
        "groq" => "Groq",
        "ollama" => "Ollama (local)",
        "local" => "Local (llama.cpp)",
        other => other,
    }
    .to_string()
}

/// A selectable provider: `(name, label, ready)`.
pub type ProviderEntry = (String, String, bool);
/// `(active_provider, active_model, suggested_models, providers)`.
pub type Options = (String, String, Vec<String>, Vec<ProviderEntry>);

/// `ready` means the provider has a usable key or needs none (local/ollama).
pub fn options(c: &BlumiConfig) -> Options {
    let provider = c.llm.provider.clone();
    let model = c.llm.model.clone();
    let mut models = suggested_models(&provider);
    if !model.is_empty() && !models.iter().any(|m| m == &model) {
        models.insert(0, model.clone());
    }
    let mut providers: Vec<ProviderEntry> = c
        .providers
        .iter()
        .filter(|(name, _)| name.as_str() != "mock")
        .map(|(name, pc)| {
            let ready =
                pc.resolve_api_key().is_some() || matches!(name.as_str(), "local" | "ollama");
            (name.clone(), provider_label(name), ready)
        })
        .collect();
    // The active provider is always selectable, even if it looks unready.
    if let Some(p) = providers.iter_mut().find(|p| p.0 == provider) {
        p.2 = true;
    }
    (provider, model, models, providers)
}

/// Persist the active provider + a default model (+ an optional key) to
/// settings.json (atomic, 0600).
pub fn persist_provider(settings: &Path, provider: &str, key: Option<&str>) -> anyhow::Result<()> {
    let default_model = suggested_models(provider)
        .into_iter()
        .next()
        .unwrap_or_default();
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    merge(settings, |root| {
        set_path(root, &["llm", "provider"], json!(provider));
        set_path(root, &["llm", "model"], json!(default_model));
        if let Some(k) = key {
            set_path(root, &["providers", provider, "api_key"], json!(k));
        }
    })
}

fn set_path(root: &mut Value, path: &[&str], val: Value) {
    let mut cur = root;
    for key in &path[..path.len() - 1] {
        if !cur[*key].is_object() {
            cur[*key] = json!({});
        }
        cur = &mut cur[*key];
    }
    cur[path[path.len() - 1]] = val;
}

fn merge(path: &Path, edit: impl FnOnce(&mut Value)) -> anyhow::Result<()> {
    let mut root: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    edit(&mut root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let body = serde_json::to_string_pretty(&root)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_and_labels() {
        assert!(suggested_models("openai").contains(&"gpt-4o".to_string()));
        assert!(suggested_models("local").is_empty());
        assert_eq!(provider_label("anthropic"), "Anthropic (Claude)");
    }

    #[test]
    fn persist_writes_provider_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        persist_provider(&path, "openai", Some("sk-x")).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["llm"]["provider"], "openai");
        assert_eq!(v["llm"]["model"], "gpt-4o");
        assert_eq!(v["providers"]["openai"]["api_key"], "sk-x");
        // No key given → don't write one.
        persist_provider(&path, "local", None).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["llm"]["provider"], "local");
        assert!(v["providers"]["local"].get("api_key").is_none());
    }
}
