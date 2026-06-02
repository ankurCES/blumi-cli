//! First-run / `blumi login` onboarding: build the provider menu, run the TUI
//! wizard, persist the choice, and reload config.

use blumi_config::{BlumiConfig, ProviderKind};
use blumi_tui::ProviderChoice;

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
