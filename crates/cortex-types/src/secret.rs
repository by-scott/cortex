use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSink {
    RuntimeBroker,
    ProviderRequest,
    WebRequest,
    PluginInput,
    PluginOutput,
    ChannelMessage,
    MemoryStore,
    Log,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretHandle {
    pub handle: String,
    pub source: String,
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_sinks: Vec<SecretSink>,
}

impl SecretHandle {
    #[must_use]
    pub fn new(
        handle: impl Into<String>,
        source: impl Into<String>,
        purpose: impl Into<String>,
    ) -> Self {
        Self {
            handle: handle.into(),
            source: source.into(),
            purpose: purpose.into(),
            allowed_sinks: vec![SecretSink::RuntimeBroker],
        }
    }

    #[must_use]
    pub fn reference(&self) -> SecretReference<'_> {
        SecretReference {
            handle: &self.handle,
            source: &self.source,
            purpose: &self.purpose,
        }
    }

    #[must_use]
    pub fn with_allowed_sink(mut self, sink: SecretSink) -> Self {
        if !self.allowed_sinks.contains(&sink) {
            self.allowed_sinks.push(sink);
        }
        self
    }

    #[must_use]
    pub fn allows_sink(&self, sink: SecretSink) -> bool {
        self.allowed_sinks.contains(&sink)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretReference<'a> {
    pub handle: &'a str,
    pub source: &'a str,
    pub purpose: &'a str,
}

impl SecretReference<'_> {
    #[must_use]
    pub fn render_for_model(&self) -> String {
        format!(
            "secret://{} purpose={} source={}",
            self.handle, self.purpose, self.source
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretSinkDecision {
    pub sink: SecretSink,
    pub allowed: bool,
    pub reason: String,
}

impl SecretSinkDecision {
    #[must_use]
    pub fn allow(sink: SecretSink, reason: impl Into<String>) -> Self {
        Self {
            sink,
            allowed: true,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn block(sink: SecretSink, reason: impl Into<String>) -> Self {
        Self {
            sink,
            allowed: false,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretSinkPolicy {
    pub default_allowed_sinks: Vec<SecretSink>,
}

impl Default for SecretSinkPolicy {
    fn default() -> Self {
        Self {
            default_allowed_sinks: vec![SecretSink::RuntimeBroker],
        }
    }
}

impl SecretSinkPolicy {
    #[must_use]
    pub fn decision(&self, handle: &SecretHandle, sink: SecretSink) -> SecretSinkDecision {
        if handle.allows_sink(sink) || self.default_allowed_sinks.contains(&sink) {
            return SecretSinkDecision::allow(sink, "secret sink explicitly allowed");
        }
        SecretSinkDecision::block(sink, "secret sink blocked by dataflow policy")
    }
}
