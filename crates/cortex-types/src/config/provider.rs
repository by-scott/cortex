use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{ApiConfig, ApiEndpointConfig, LlmGroupConfig, inferred_model_token_limits};

/// Provider registry: maps provider name to its configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRegistry(HashMap<String, ProviderConfig>);

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, name: String, config: ProviderConfig) {
        self.0.insert(name, config);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ProviderConfig> {
        self.0.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ProviderConfig> {
        self.0.get_mut(name)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Find a provider whose `base_url` contains the given URL (or vice versa).
    #[must_use]
    pub fn find_by_url(&self, url: &str) -> Option<(String, &ProviderConfig)> {
        let normalized = url.trim_end_matches('/');
        self.0.iter().find_map(|(name, cfg)| {
            let cfg_url = cfg.base_url.trim_end_matches('/');
            if cfg_url == normalized
                || normalized.starts_with(cfg_url)
                || cfg_url.starts_with(normalized)
            {
                Some((name.clone(), cfg))
            } else {
                None
            }
        })
    }

    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ProviderConfig)> {
        self.0.iter()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub auth_type: AuthType,
    pub models: Vec<String>,
    /// Provider used for vision requests. Empty = use this provider.
    ///
    /// This keeps multimodal routing explicit. Some gateways expose a good
    /// text endpoint and a separate OpenAI-compatible vision endpoint.
    pub vision_provider: String,
    /// Default vision model for this provider. Empty = auto-discovery.
    pub vision_model: String,
    /// How OpenAI-compatible multimodal image blocks should be sent.
    pub image_input_mode: OpenAiImageInputMode,
    /// Optional base URL used for file upload/content URLs when
    /// `image_input_mode = "upload-then-url"`.
    pub files_base_url: String,
    /// Whether this OpenAI-compatible endpoint accepts `stream_options`.
    ///
    /// Many gateways implement chat-completions streaming but reject `OpenAI`'s
    /// optional usage extension. Keep this explicit instead of hard-coding
    /// provider names in the LLM client.
    pub openai_stream_options: bool,
    /// Optional request parameter used by OpenAI-compatible endpoints to
    /// enable/disable model thinking. vLLM exposes this through
    /// `chat_template_kwargs` rather than the standard `OpenAI` schema.
    pub openai_thinking_parameter: OpenAiThinkingParameter,
    /// Provider-specific maximum output tokens for multimodal/vision requests.
    /// `0` means use `DEFAULT_VISION_MAX_OUTPUT_TOKENS`.
    pub vision_max_output_tokens: usize,
    /// Capability cache TTL in hours. `0` means use the runtime default.
    pub capability_cache_ttl_hours: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAiImageInputMode {
    #[default]
    DataUrl,
    UploadThenUrl,
    RemoteUrlOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAiThinkingParameter {
    #[default]
    None,
    TopLevelThinking,
    ChatTemplateThinking,
    ChatTemplateEnableThinking,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderProtocol {
    Anthropic,
    #[default]
    #[serde(rename = "openai")]
    OpenAI,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTokenLimits {
    pub context_tokens: usize,
    pub output_tokens: usize,
}

impl ModelTokenLimits {
    #[must_use]
    pub const fn new(context_tokens: usize, output_tokens: usize) -> Self {
        Self {
            context_tokens,
            output_tokens,
        }
    }

    #[must_use]
    pub const fn with_overrides(self, context_tokens: usize, output_tokens: usize) -> Self {
        Self {
            context_tokens: if context_tokens == 0 {
                self.context_tokens
            } else {
                context_tokens
            },
            output_tokens: if output_tokens == 0 {
                self.output_tokens
            } else {
                output_tokens
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthType {
    XApiKey,
    #[default]
    Bearer,
    None,
}

/// Fully resolved API endpoint after fallback chain.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub provider: String,
    pub base_url: String,
    pub protocol: ProviderProtocol,
    pub auth_type: AuthType,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
    pub vision_max_output_tokens: usize,
    pub image_input_mode: OpenAiImageInputMode,
    pub files_base_url: String,
    pub openai_stream_options: bool,
    pub openai_thinking_parameter: OpenAiThinkingParameter,
    pub capability_cache_path: String,
    pub capability_cache_ttl_hours: u64,
}

impl ResolvedEndpoint {
    fn from_provider(
        provider_name: &str,
        provider: &ProviderConfig,
        api_key: String,
        model: String,
        max_tokens: usize,
    ) -> Self {
        Self {
            provider: provider_name.to_string(),
            base_url: provider.base_url.clone(),
            protocol: provider.protocol.clone(),
            auth_type: provider.auth_type.clone(),
            api_key,
            model,
            max_tokens,
            vision_max_output_tokens: provider.vision_max_output_tokens,
            image_input_mode: provider.image_input_mode.clone(),
            files_base_url: provider.files_base_url.clone(),
            openai_stream_options: provider.openai_stream_options,
            openai_thinking_parameter: provider.openai_thinking_parameter.clone(),
            capability_cache_path: String::new(),
            capability_cache_ttl_hours: provider.capability_cache_ttl_hours,
        }
    }

    #[must_use]
    pub fn with_capability_cache_path(mut self, path: String) -> Self {
        self.capability_cache_path = path;
        self
    }

    #[must_use]
    pub const fn supports_direct_image_input(&self) -> bool {
        match self.protocol {
            ProviderProtocol::Anthropic | ProviderProtocol::Ollama => true,
            ProviderProtocol::OpenAI => matches!(
                self.image_input_mode,
                OpenAiImageInputMode::DataUrl | OpenAiImageInputMode::UploadThenUrl
            ),
        }
    }

    #[must_use]
    pub const fn requires_remote_image_url(&self) -> bool {
        matches!(self.image_input_mode, OpenAiImageInputMode::RemoteUrlOnly)
    }
}

impl ResolvedEndpoint {
    /// Resolve a sub-config endpoint by filling empty fields from parent `[api]`,
    /// then looking up provider in the registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve(
        endpoint: &ApiEndpointConfig,
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Self, String> {
        Self::resolve_with_groups(endpoint, parent, providers, &HashMap::new())
    }

    /// Resolve an endpoint with group inheritance.
    ///
    /// Priority: endpoint field > group field > parent `[api]` field > default.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_with_groups(
        endpoint: &ApiEndpointConfig,
        parent: &ApiConfig,
        providers: &ProviderRegistry,
        groups: &HashMap<String, LlmGroupConfig>,
    ) -> Result<Self, String> {
        let group = if endpoint.group.is_empty() {
            None
        } else {
            groups.get(&endpoint.group)
        };

        let provider_name = if !endpoint.provider.is_empty() {
            &endpoint.provider
        } else if let Some(g) = group
            && !g.provider.is_empty()
        {
            &g.provider
        } else {
            &parent.provider
        };
        let api_key = if !endpoint.api_key.is_empty() {
            endpoint.api_key.clone()
        } else if let Some(g) = group
            && !g.api_key.is_empty()
        {
            g.api_key.clone()
        } else {
            parent.api_key.clone()
        };

        let configured_model = if !endpoint.model.is_empty() {
            endpoint.model.clone()
        } else if let Some(g) = group
            && !g.model.is_empty()
        {
            g.model.clone()
        } else {
            parent.model.clone()
        };

        let configured_max_tokens = if endpoint.max_tokens > 0 {
            Some(endpoint.max_tokens)
        } else if let Some(g) = group
            && g.max_tokens > 0
        {
            Some(g.max_tokens)
        } else if parent.max_tokens > 0 {
            Some(parent.max_tokens)
        } else {
            None
        };
        let provider = providers
            .get(provider_name)
            .ok_or_else(|| format!("provider not found: {provider_name}"))?;
        let model = resolved_model_name(&configured_model, provider);
        let max_tokens = configured_max_tokens.unwrap_or_else(|| {
            inferred_model_token_limits(provider_name, &provider.protocol, &model).output_tokens
        });
        Ok(Self::from_provider(
            provider_name,
            provider,
            api_key,
            model,
            max_tokens,
        ))
    }

    /// Resolve the vision model override, returning `None` if not configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_vision(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Option<Self>, String> {
        if parent.vision.model.is_empty() && parent.vision.provider.is_empty() {
            return Ok(None);
        }
        let endpoint = ApiEndpointConfig {
            provider: parent.vision.provider.clone(),
            model: parent.vision.model.clone(),
            ..ApiEndpointConfig::default()
        };
        Self::resolve(&endpoint, parent, providers).map(Some)
    }

    /// Resolve the primary `[api]` config directly.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_primary(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Self, String> {
        let api_key = parent.api_key.clone();
        let provider = providers
            .get(&parent.provider)
            .ok_or_else(|| format!("provider not found: {}", parent.provider))?;
        let model = resolved_model_name(&parent.model, provider);
        let max_tokens = if parent.max_tokens == 0 {
            inferred_model_token_limits(&parent.provider, &provider.protocol, &model).output_tokens
        } else {
            parent.max_tokens
        };
        Ok(Self::from_provider(
            &parent.provider,
            provider,
            api_key,
            model,
            max_tokens,
        ))
    }

    /// Resolve the best vision endpoint for the primary API route.
    ///
    /// Priority:
    /// 1. Explicit `[api.vision]`.
    /// 2. Provider-declared `vision_provider`.
    /// 3. Provider-declared `vision_model` on the primary provider.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicitly referenced provider is missing.
    pub fn resolve_vision_endpoint(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Option<Self>, String> {
        if let Some(endpoint) = Self::resolve_vision(parent, providers)? {
            return Ok(Some(endpoint));
        }

        let primary_provider = providers
            .get(&parent.provider)
            .ok_or_else(|| format!("provider not found: {}", parent.provider))?;

        if !primary_provider.vision_provider.is_empty() {
            let vision_provider_name = &primary_provider.vision_provider;
            let vision_provider = providers
                .get(vision_provider_name)
                .ok_or_else(|| format!("provider not found: {vision_provider_name}"))?;
            if !vision_provider.vision_model.is_empty() {
                let max_tokens = if parent.max_tokens == 0 {
                    inferred_model_token_limits(
                        vision_provider_name,
                        &vision_provider.protocol,
                        &vision_provider.vision_model,
                    )
                    .output_tokens
                } else {
                    parent.max_tokens
                };
                return Ok(Some(Self::from_provider(
                    vision_provider_name,
                    vision_provider,
                    parent.api_key.clone(),
                    vision_provider.vision_model.clone(),
                    max_tokens,
                )));
            }
        }

        if !primary_provider.vision_model.is_empty() {
            let max_tokens = if parent.max_tokens == 0 {
                inferred_model_token_limits(
                    &parent.provider,
                    &primary_provider.protocol,
                    &primary_provider.vision_model,
                )
                .output_tokens
            } else {
                parent.max_tokens
            };
            return Ok(Some(Self::from_provider(
                &parent.provider,
                primary_provider,
                parent.api_key.clone(),
                primary_provider.vision_model.clone(),
                max_tokens,
            )));
        }

        Ok(None)
    }
}

fn resolved_model_name(configured_model: &str, provider: &ProviderConfig) -> String {
    if configured_model.is_empty() {
        provider.models.first().cloned().unwrap_or_default()
    } else {
        configured_model.to_string()
    }
}
