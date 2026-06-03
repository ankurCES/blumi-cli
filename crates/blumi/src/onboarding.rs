//! First-run / `blumi login` onboarding: build the provider menu, run the TUI
//! wizard, persist the choice, then (optionally) configure voice + a messaging
//! gateway via simple prompts, and reload config.

use blumi_config::{BlumiConfig, Paths, ProviderKind};
use blumi_tui::ProviderChoice;
use serde_json::json;
use std::io::{self, Write};
use std::path::Path;

/// Run the wizard if needed (first run, or `force`), persist the result, and
/// return the reloaded config. `Ok(None)` means the user cancelled.
pub async fn ensure_configured(
    config: BlumiConfig,
    force: bool,
) -> anyhow::Result<Option<BlumiConfig>> {
    if !force && !config.is_first_run() {
        return Ok(Some(config));
    }

    let choices = provider_choices(&config);
    let working_dir = config.paths.working_dir.clone();
    let home = config.paths.home.clone();

    match blumi_tui::run_onboarding(choices).await? {
        Some(o) => {
            config.paths.ensure_dirs().ok();
            blumi_config::write_provider_setup(
                &config.paths,
                &o.provider,
                o.kind,
                &o.model,
                o.endpoint.as_deref(),
                o.api_key.as_deref(),
            )?;
            // Optional extras (voice + a gateway). Best-effort: a non-interactive
            // stdin (e.g. piped) just skips it without failing onboarding.
            if let Err(e) = configure_extras(&config.paths) {
                tracing::debug!("skipped voice/gateway setup: {e}");
            }
            let reloaded = BlumiConfig::load(working_dir, Some(home))?;
            Ok(Some(reloaded))
        }
        None => Ok(None),
    }
}

fn provider_choices(config: &BlumiConfig) -> Vec<ProviderChoice> {
    // Curated display order; only providers present in config (gemini/mock omitted).
    const ORDER: [&str; 10] = [
        "anthropic",
        "azure-foundry",
        "openai",
        "gemini",
        "openrouter",
        "deepseek",
        "minimax",
        "groq",
        "ollama",
        "local",
    ];
    ORDER
        .iter()
        .filter_map(|&name| {
            let p = config.providers.get(name)?;
            let needs_endpoint = matches!(p.kind, ProviderKind::AnthropicFoundry);
            let needs_key = !matches!(name, "local" | "ollama");
            Some(ProviderChoice {
                name: name.to_string(),
                label: label_for(name).to_string(),
                kind: p.kind,
                fixed_base_url: if needs_endpoint {
                    None
                } else {
                    p.base_url.clone()
                },
                needs_key,
                needs_endpoint,
                model_hint: hint_for(name).to_string(),
            })
        })
        .collect()
}

fn label_for(name: &str) -> &str {
    match name {
        "anthropic" => "Anthropic (Claude)",
        "azure-foundry" => "Azure AI Foundry (Claude)",
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
}

fn hint_for(name: &str) -> &str {
    match name {
        "anthropic" => "claude-sonnet-4-5",
        "azure-foundry" => "your Claude deployment name",
        "openai" => "gpt-4o, o4-mini",
        "gemini" => "gemini-2.0-flash",
        "openrouter" => "anthropic/claude-3.7-sonnet",
        "deepseek" => "deepseek-chat",
        "minimax" => "MiniMax-Text-01",
        "groq" => "llama-3.3-70b-versatile",
        "ollama" => "llama3.1",
        "local" => "(server default)",
        _ => "",
    }
}

// ── Optional extras: voice + a messaging gateway ────────────────────────────

/// After the provider wizard, offer to set up voice (STT + a TTS provider,
/// including ElevenLabs) and a messaging gateway, writing into settings.json.
fn configure_extras(paths: &Paths) -> anyhow::Result<()> {
    println!("\n✿ Optional: voice + messaging gateways (you can edit settings.json later).");
    if !prompt_yes_no("  Configure these now?", false)? {
        return Ok(());
    }
    let settings = paths.settings_json();

    if prompt_yes_no("\n  Enable voice (speech-to-text + text-to-speech)?", false)? {
        let stt_url = prompt("    STT base URL", "https://api.openai.com/v1")?;
        let stt_model = prompt("    STT model", "whisper-1")?;
        let stt_key = prompt("    STT API key (blank for a local server)", "")?;
        let provider = {
            let p = prompt("    TTS provider (openai/elevenlabs)", "openai")?;
            if p.eq_ignore_ascii_case("elevenlabs") {
                "elevenlabs"
            } else {
                "openai"
            }
        };
        // Provider-specific TTS fields.
        let (tts_base, tts_model, tts_voice, tts_key) = if provider == "elevenlabs" {
            (
                String::new(),
                prompt("    ElevenLabs model", "eleven_multilingual_v2")?,
                prompt("    ElevenLabs voice id", "21m00Tcm4TlvDq8ikWAM")?,
                prompt("    ElevenLabs API key", "")?,
            )
        } else {
            (
                prompt("    TTS base URL", &stt_url)?,
                prompt("    TTS model", "tts-1")?,
                prompt("    TTS voice", "alloy")?,
                String::new(),
            )
        };
        merge_settings(&settings, |root| {
            root["voice"]["enabled"] = json!(true);
            root["voice"]["api_key"] = json!(stt_key);
            root["voice"]["stt_base_url"] = json!(stt_url);
            root["voice"]["stt_model"] = json!(stt_model);
            root["voice"]["tts_provider"] = json!(provider);
            root["voice"]["tts_base_url"] = json!(tts_base);
            root["voice"]["tts_model"] = json!(tts_model);
            root["voice"]["tts_voice"] = json!(tts_voice);
            root["voice"]["tts_api_key"] = json!(tts_key);
        })?;
        println!("    ✓ voice configured");
    }

    if prompt_yes_no(
        "\n  Set up a messaging gateway (run blumi as a bot)?",
        false,
    )? {
        let platform =
            prompt("    Platform (telegram/discord/slack/whatsapp)", "telegram")?.to_lowercase();
        match platform.as_str() {
            "telegram" => {
                let token = prompt("    Telegram bot token (from @BotFather)", "")?;
                merge_settings(&settings, |r| {
                    r["gateway"]["telegram"]["token"] = json!(token);
                })?;
            }
            "discord" => {
                let token = prompt("    Discord bot token", "")?;
                merge_settings(&settings, |r| {
                    r["gateway"]["discord"]["token"] = json!(token);
                })?;
            }
            "slack" => {
                let bot = prompt("    Slack bot token (xoxb-…)", "")?;
                let app = prompt("    Slack app token (xapp-…)", "")?;
                merge_settings(&settings, |r| {
                    r["gateway"]["slack"]["bot_token"] = json!(bot);
                    r["gateway"]["slack"]["app_token"] = json!(app);
                })?;
            }
            "whatsapp" => {
                let token = prompt("    WhatsApp access token", "")?;
                let phone = prompt("    WhatsApp phone_number_id", "")?;
                let verify = prompt("    Webhook verify token (you choose)", "blumi-verify")?;
                merge_settings(&settings, |r| {
                    r["gateway"]["whatsapp"]["token"] = json!(token);
                    r["gateway"]["whatsapp"]["phone_number_id"] = json!(phone);
                    r["gateway"]["whatsapp"]["verify_token"] = json!(verify);
                })?;
            }
            other => {
                println!("    (unknown platform '{other}', skipped)");
                return Ok(());
            }
        }
        // Gateways auto-deny tool approvals by default; offer to allow them.
        if prompt_yes_no(
            "    Auto-approve tool calls in the bot (needs a sandbox)?",
            false,
        )? {
            merge_settings(&settings, |r| r["gateway"]["yolo"] = json!(true))?;
        }
        println!("    ✓ gateway configured — start it with: blumi gateway {platform}");
    }

    Ok(())
}

/// Read a line; returns `default` when the input is empty.
fn prompt(label: &str, default: &str) -> io::Result<String> {
    if default.is_empty() {
        print!("{label}: ");
    } else {
        print!("{label} [{default}]: ");
    }
    io::stdout().flush()?;
    let mut s = String::new();
    if io::stdin().read_line(&mut s)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "no input (non-interactive)",
        ));
    }
    let s = s.trim();
    Ok(if s.is_empty() {
        default.to_string()
    } else {
        s.to_string()
    })
}

fn prompt_yes_no(label: &str, default_yes: bool) -> io::Result<bool> {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    print!("{label} [{hint}]: ");
    io::stdout().flush()?;
    let mut s = String::new();
    if io::stdin().read_line(&mut s)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "no input (non-interactive)",
        ));
    }
    Ok(match s.trim().to_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    })
}

/// Read settings.json (or `{}`), apply `edit`, write back atomically (0600).
fn merge_settings(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) -> anyhow::Result<()> {
    let mut root: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
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
    fn merge_writes_a_loadable_voice_and_gateway_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Apply the same edits the onboarding prompts produce.
        merge_settings(&path, |r| {
            r["voice"]["enabled"] = json!(true);
            r["voice"]["tts_provider"] = json!("elevenlabs");
            r["voice"]["tts_api_key"] = json!("xi-key");
            r["gateway"]["telegram"]["token"] = json!("123:abc");
        })
        .unwrap();
        // Merging again must preserve existing keys.
        merge_settings(&path, |r| r["gateway"]["yolo"] = json!(true)).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["voice"]["tts_provider"], "elevenlabs");
        assert_eq!(root["gateway"]["telegram"]["token"], "123:abc");

        // The onboarding-written shape must deserialize as a real BlumiConfig.
        let cfg: BlumiConfig = serde_json::from_value(root).unwrap();
        assert!(cfg.voice.enabled);
        assert_eq!(cfg.voice.tts_provider, "elevenlabs");
        assert_eq!(cfg.voice.tts_api_key, "xi-key");
        assert_eq!(cfg.gateway.telegram.token, "123:abc");
        assert!(cfg.gateway.yolo);
    }
}
