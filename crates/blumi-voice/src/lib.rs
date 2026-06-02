//! Voice: speech-to-text (whisper) and text-to-speech over OpenAI-compatible
//! audio endpoints (`/audio/transcriptions`, `/audio/speech`). Works with
//! OpenAI, Groq, a local whisper.cpp/`speaches` server — anything that speaks
//! the same shapes — selected purely by `base_url` + `api_key` (no OAuth).

use serde_json::json;

/// Where to reach the STT/TTS services. Empty `api_key` is allowed for local
/// servers that don't require one.
#[derive(Debug, Clone, Default)]
pub struct VoiceConfig {
    pub api_key: String,
    /// Base URL for transcription, e.g. `https://api.openai.com/v1`.
    pub stt_base_url: String,
    pub stt_model: String,
    /// Base URL for speech synthesis.
    pub tts_base_url: String,
    pub tts_model: String,
    pub tts_voice: String,
}

impl VoiceConfig {
    /// True if transcription looks configured.
    pub fn stt_ready(&self) -> bool {
        !self.stt_base_url.trim().is_empty() && !self.stt_model.trim().is_empty()
    }
    /// True if synthesis looks configured.
    pub fn tts_ready(&self) -> bool {
        !self.tts_base_url.trim().is_empty() && !self.tts_model.trim().is_empty()
    }
}

/// Join a base URL and a path with exactly one slash.
pub fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Transcribe audio bytes to text. `filename` should carry the real extension
/// (e.g. `audio.webm`) so the server can detect the format.
pub async fn transcribe(
    cfg: &VoiceConfig,
    audio: Vec<u8>,
    filename: &str,
    mime: &str,
) -> anyhow::Result<String> {
    if !cfg.stt_ready() {
        anyhow::bail!("transcription isn't configured (set voice.stt_base_url + voice.stt_model)");
    }
    let part = reqwest::multipart::Part::bytes(audio)
        .file_name(filename.to_string())
        .mime_str(mime)
        .unwrap_or_else(|_| {
            reqwest::multipart::Part::bytes(Vec::new()).file_name(filename.to_string())
        });
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", cfg.stt_model.clone());

    let resp = reqwest::Client::new()
        .post(endpoint(&cfg.stt_base_url, "audio/transcriptions"))
        .bearer_auth(&cfg.api_key)
        .multipart(form)
        .send()
        .await?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await?;
    if !status.is_success() {
        anyhow::bail!("transcription failed ({status}): {body}");
    }
    body["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("no text in transcription response"))
}

/// Synthesize speech from text, returning the audio bytes (typically MP3).
pub async fn synthesize(cfg: &VoiceConfig, text: &str) -> anyhow::Result<Vec<u8>> {
    if !cfg.tts_ready() {
        anyhow::bail!(
            "speech synthesis isn't configured (set voice.tts_base_url + voice.tts_model)"
        );
    }
    let voice = if cfg.tts_voice.trim().is_empty() {
        "alloy"
    } else {
        cfg.tts_voice.trim()
    };
    let resp = reqwest::Client::new()
        .post(endpoint(&cfg.tts_base_url, "audio/speech"))
        .bearer_auth(&cfg.api_key)
        .json(&json!({ "model": cfg.tts_model, "voice": voice, "input": text }))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("speech synthesis failed ({status}): {body}");
    }
    Ok(resp.bytes().await?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_joins_with_one_slash() {
        assert_eq!(
            endpoint("https://api.openai.com/v1/", "/audio/speech"),
            "https://api.openai.com/v1/audio/speech"
        );
        assert_eq!(
            endpoint("http://localhost:8000/v1", "audio/transcriptions"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
    }

    #[test]
    fn readiness_checks() {
        let mut c = VoiceConfig::default();
        assert!(!c.stt_ready() && !c.tts_ready());
        c.stt_base_url = "x".into();
        c.stt_model = "whisper-1".into();
        assert!(c.stt_ready());
        assert!(!c.tts_ready());
    }
}
