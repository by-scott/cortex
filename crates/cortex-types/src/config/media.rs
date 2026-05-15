use serde::{Deserialize, Serialize};

/// Configuration for media capabilities with provider-based dispatch.
///
/// Each capability (`stt`, `tts`, `image_gen`, `image_understand`, `video_gen`,
/// `video_understand`) specifies a provider name.  An empty string disables
/// that capability (or uses a built-in fallback where applicable).
///
/// Provider names:
/// - STT: `"local"` (whisper CLI), `"openai"`, `"zai"`
/// - TTS: `"edge"` (edge-tts CLI), `"openai"`, `"zai"`
/// - Image gen: `"zai"`, `"openai"`, `""` (disabled)
/// - Image understand: `"zai"`, `"openai"`, `""` (default = main LLM vision)
/// - Video gen: `"zai"`, `""` (disabled)
/// - Video understand: `"zai"`, `"gemini"`, `""` (disabled)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// STT provider: `"local"` (whisper CLI), `"openai"`, `"zai"`.
    pub stt: String,
    /// TTS provider: `"edge"` (edge-tts CLI), `"openai"`, `"zai"`.
    pub tts: String,
    /// Image generation provider: `"zai"`, `"openai"`, `""` (disabled).
    pub image_gen: String,
    /// Video generation provider: `"zai"`, `""` (disabled).
    pub video_gen: String,
    /// Image understanding provider: `"zai"`, `"openai"`, `""` (use main LLM vision).
    ///
    /// Default empty = main LLM handles vision natively (recommended).
    /// Only set a provider when you want a dedicated vision model.
    pub image_understand: String,
    /// Image understanding API key override.
    #[serde(default)]
    pub image_understand_api_key: String,
    /// Image understanding API URL override.
    #[serde(default)]
    pub image_understand_api_url: String,
    /// Image understanding model name (empty = provider default).
    pub image_understand_model: String,

    /// Video understanding provider: `"zai"`, `"gemini"`, `""` (disabled).
    pub video_understand: String,

    /// Shared API key for media providers (empty = inherit from `[api].api_key`).
    pub api_key: String,
    /// Shared API URL override (empty = use provider default).
    pub api_url: String,

    // ── Per-capability overrides (empty = inherit shared/[api]) ──
    /// STT API key override.
    #[serde(default)]
    pub stt_api_key: String,
    /// STT API URL override.
    #[serde(default)]
    pub stt_api_url: String,
    /// Local whisper model name (for `stt = "local"`).
    pub whisper_model: String,

    /// TTS API key override.
    #[serde(default)]
    pub tts_api_key: String,
    /// TTS API URL override.
    #[serde(default)]
    pub tts_api_url: String,
    /// TTS voice identifier (e.g. `"zh-CN-XiaoxiaoNeural"` for edge).
    pub tts_voice: String,

    /// Image generation API key override.
    #[serde(default)]
    pub image_gen_api_key: String,
    /// Image generation API URL override.
    #[serde(default)]
    pub image_gen_api_url: String,
    /// Image generation model name.
    pub image_gen_model: String,

    /// Video generation API key override.
    #[serde(default)]
    pub video_gen_api_key: String,
    /// Video generation API URL override.
    #[serde(default)]
    pub video_gen_api_url: String,
    /// Video generation model name (default: `"cogvideox-3"`).
    pub video_gen_model: String,

    /// Video understanding API key override.
    #[serde(default)]
    pub video_understand_api_key: String,
    /// Video understanding API URL override.
    #[serde(default)]
    pub video_understand_api_url: String,
    /// Video understanding model name (default: `"glm-4v-plus"`).
    pub video_understand_model: String,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            stt: "local".into(),
            tts: "edge".into(),
            image_gen: String::new(),
            image_understand: String::new(),
            image_understand_api_key: String::new(),
            image_understand_api_url: String::new(),
            image_understand_model: String::new(),
            video_gen: String::new(),
            video_understand: String::new(),
            api_key: String::new(),
            api_url: String::new(),
            stt_api_key: String::new(),
            stt_api_url: String::new(),
            whisper_model: "whisper".into(),
            tts_api_key: String::new(),
            tts_api_url: String::new(),
            tts_voice: "zh-CN-XiaoxiaoNeural".into(),
            image_gen_api_key: String::new(),
            image_gen_api_url: String::new(),
            image_gen_model: String::new(),
            video_gen_api_key: String::new(),
            video_gen_api_url: String::new(),
            video_gen_model: "cogvideox-3".into(),
            video_understand_api_key: String::new(),
            video_understand_api_url: String::new(),
            video_understand_model: "glm-4v-plus".into(),
        }
    }
}

impl MediaConfig {
    /// Resolve API key: `capability_key` > `media.api_key` > `global_fallback`.
    #[must_use]
    pub fn resolve_key<'a>(&'a self, capability_key: &'a str, global_fallback: &'a str) -> &'a str {
        first_non_empty(&[capability_key, &self.api_key, global_fallback])
    }

    /// Resolve API URL: `capability_url` > `media.api_url` > `provider_default`.
    #[must_use]
    pub fn resolve_url<'a>(
        &'a self,
        capability_url: &'a str,
        provider_default: &'a str,
    ) -> &'a str {
        first_non_empty(&[capability_url, &self.api_url, provider_default])
    }

    /// Shorthand for STT key resolution.
    #[must_use]
    pub fn stt_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.stt_api_key, global)
    }

    /// Shorthand for STT URL resolution.
    #[must_use]
    pub fn stt_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.stt_api_url, default)
    }

    /// Shorthand for TTS key resolution.
    #[must_use]
    pub fn tts_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.tts_api_key, global)
    }

    /// Shorthand for TTS URL resolution.
    #[must_use]
    pub fn tts_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.tts_api_url, default)
    }

    /// Shorthand for image generation key resolution.
    #[must_use]
    pub fn image_gen_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.image_gen_api_key, global)
    }

    /// Shorthand for image generation URL resolution.
    #[must_use]
    pub fn image_gen_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.image_gen_api_url, default)
    }

    /// Shorthand for video generation key resolution.
    #[must_use]
    pub fn video_gen_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.video_gen_api_key, global)
    }

    /// Shorthand for video generation URL resolution.
    #[must_use]
    pub fn video_gen_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.video_gen_api_url, default)
    }

    /// Shorthand for image understanding key resolution.
    #[must_use]
    pub fn image_understand_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.image_understand_api_key, global)
    }

    /// Shorthand for image understanding URL resolution.
    #[must_use]
    pub fn image_understand_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.image_understand_api_url, default)
    }

    /// Shorthand for video understanding key resolution.
    #[must_use]
    pub fn video_understand_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.video_understand_api_key, global)
    }

    /// Shorthand for video understanding URL resolution.
    #[must_use]
    pub fn video_understand_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.video_understand_api_url, default)
    }
}

/// Return the first non-empty string from the candidates.
fn first_non_empty<'a>(candidates: &[&'a str]) -> &'a str {
    candidates
        .iter()
        .copied()
        .find(|s| !s.is_empty())
        .unwrap_or("")
}
