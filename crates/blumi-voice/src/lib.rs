//! Voice: speech-to-text (whisper) and text-to-speech.
//!
//! STT is OpenAI-compatible (`/audio/transcriptions`) — OpenAI, Groq, a local
//! whisper.cpp/`speaches` server, anything with the same shape. TTS supports two
//! providers: OpenAI-compatible (`/audio/speech`) and **ElevenLabs**. All are
//! selected by `base_url` + `api_key` (API-key auth, no OAuth).

use serde_json::json;

/// ElevenLabs defaults used when the corresponding fields are blank.
const ELEVENLABS_BASE: &str = "https://api.elevenlabs.io/v1";
const ELEVENLABS_VOICE: &str = "21m00Tcm4TlvDq8ikWAM"; // "Rachel"
const ELEVENLABS_MODEL: &str = "eleven_multilingual_v2";

/// Where to reach the STT/TTS services. Empty `api_key` is allowed for local
/// servers that don't require one.
#[derive(Debug, Clone, Default)]
pub struct VoiceConfig {
    /// API key for transcription (and TTS, unless `tts_api_key` is set).
    pub api_key: String,
    /// Base URL for transcription, e.g. `https://api.openai.com/v1`.
    pub stt_base_url: String,
    pub stt_model: String,
    /// TTS provider: `"openai"` (default, OpenAI-compatible) or `"elevenlabs"`.
    pub tts_provider: String,
    /// Base URL for speech synthesis (provider default used when blank).
    pub tts_base_url: String,
    pub tts_model: String,
    /// Voice name (OpenAI) or voice id (ElevenLabs).
    pub tts_voice: String,
    /// Separate key for TTS (falls back to `api_key` when blank) — handy when
    /// STT and TTS use different providers.
    pub tts_api_key: String,
}

impl VoiceConfig {
    /// True if the TTS provider is ElevenLabs.
    fn is_elevenlabs(&self) -> bool {
        self.tts_provider.eq_ignore_ascii_case("elevenlabs")
    }

    /// The key to use for TTS (TTS-specific if set, else the shared key).
    fn tts_key(&self) -> &str {
        if self.tts_api_key.trim().is_empty() {
            &self.api_key
        } else {
            &self.tts_api_key
        }
    }

    /// True if transcription looks configured.
    pub fn stt_ready(&self) -> bool {
        !self.stt_base_url.trim().is_empty() && !self.stt_model.trim().is_empty()
    }

    /// True if synthesis looks configured (provider-aware — ElevenLabs only
    /// needs a key since base/model/voice have defaults).
    pub fn tts_ready(&self) -> bool {
        if self.is_elevenlabs() {
            !self.tts_key().trim().is_empty()
        } else {
            !self.tts_base_url.trim().is_empty() && !self.tts_model.trim().is_empty()
        }
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

/// Synthesize speech from text, returning the audio bytes (MP3). Dispatches to
/// the configured TTS provider.
pub async fn synthesize(cfg: &VoiceConfig, text: &str) -> anyhow::Result<Vec<u8>> {
    if !cfg.tts_ready() {
        anyhow::bail!("speech synthesis isn't configured (set voice.tts_* or an ElevenLabs key)");
    }
    if cfg.is_elevenlabs() {
        synthesize_elevenlabs(cfg, text).await
    } else {
        synthesize_openai(cfg, text).await
    }
}

/// OpenAI-compatible `/audio/speech` (bearer auth, `voice` by name).
async fn synthesize_openai(cfg: &VoiceConfig, text: &str) -> anyhow::Result<Vec<u8>> {
    let voice = if cfg.tts_voice.trim().is_empty() {
        "alloy"
    } else {
        cfg.tts_voice.trim()
    };
    let resp = reqwest::Client::new()
        .post(endpoint(&cfg.tts_base_url, "audio/speech"))
        .bearer_auth(cfg.tts_key())
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

/// ElevenLabs `/text-to-speech/{voice_id}` (`xi-api-key` header, `model_id`).
async fn synthesize_elevenlabs(cfg: &VoiceConfig, text: &str) -> anyhow::Result<Vec<u8>> {
    let base = if cfg.tts_base_url.trim().is_empty() {
        ELEVENLABS_BASE
    } else {
        cfg.tts_base_url.trim()
    };
    let voice_id = if cfg.tts_voice.trim().is_empty() {
        ELEVENLABS_VOICE
    } else {
        cfg.tts_voice.trim()
    };
    let model_id = if cfg.tts_model.trim().is_empty() {
        ELEVENLABS_MODEL
    } else {
        cfg.tts_model.trim()
    };
    let resp = reqwest::Client::new()
        .post(endpoint(base, &format!("text-to-speech/{voice_id}")))
        .header("xi-api-key", cfg.tts_key())
        .header("accept", "audio/mpeg")
        .json(&json!({ "text": text, "model_id": model_id }))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("elevenlabs synthesis failed ({status}): {body}");
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

    #[test]
    fn openai_tts_needs_base_and_model() {
        let mut c = VoiceConfig {
            tts_provider: "openai".into(),
            api_key: "k".into(),
            ..Default::default()
        };
        assert!(!c.tts_ready()); // base/model missing
        c.tts_base_url = "https://api.openai.com/v1".into();
        c.tts_model = "tts-1".into();
        assert!(c.tts_ready());
    }

    #[test]
    fn elevenlabs_tts_needs_only_a_key() {
        let mut c = VoiceConfig {
            tts_provider: "elevenlabs".into(),
            ..Default::default()
        };
        assert!(!c.tts_ready()); // no key
        c.api_key = "xi".into();
        assert!(c.tts_ready()); // base/model/voice default
        assert!(c.is_elevenlabs());
    }

    #[test]
    fn tts_key_prefers_specific_then_shared() {
        let c = VoiceConfig {
            api_key: "shared".into(),
            tts_api_key: "tts-only".into(),
            ..Default::default()
        };
        assert_eq!(c.tts_key(), "tts-only");
        let c2 = VoiceConfig {
            api_key: "shared".into(),
            ..Default::default()
        };
        assert_eq!(c2.tts_key(), "shared");
    }

    #[test]
    fn elevenlabs_url_shape() {
        assert_eq!(
            endpoint(
                ELEVENLABS_BASE,
                &format!("text-to-speech/{ELEVENLABS_VOICE}")
            ),
            "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM"
        );
    }
}
