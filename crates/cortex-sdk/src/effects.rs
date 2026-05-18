use serde::{Deserialize, Serialize};

use crate::InvocationContext;

/// Stable categories for side effects a tool may perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectKind {
    ReadFile,
    ReadSecret,
    WriteFile,
    DeleteFile,
    RunProcess,
    NetworkRequest,
    SendMessage,
    SpendMoney,
    Deploy,
    ModifyCredential,
    PersistMemory,
    PublishContent,
    ScheduleTask,
    GenerateMedia,
    IntrospectRuntime,
    DelegateWork,
}

/// Whether a declared effect can be undone after execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectReversibility {
    Reversible,
    PartiallyReversible,
    Irreversible,
}

/// When the runtime should ask for confirmation before executing an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectConfirmation {
    Never,
    OnRisk,
    Always,
}

/// Whether a tool can preview its effect before committing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DryRunSupport {
    NotSupported,
    Supported,
    RequiredBeforeExecute,
}

/// Declarative hints about how a tool participates in the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEffect {
    /// The stable effect category.
    pub kind: ToolEffectKind,
    /// Optional target, such as a path, host, channel, or resource name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// Whether this effect can be undone after execution.
    pub reversibility: EffectReversibility,
    /// Confirmation preference declared by the tool author.
    pub confirmation: EffectConfirmation,
    /// Dry-run support declared by the tool author.
    pub dry_run: DryRunSupport,
}

impl ToolEffect {
    #[must_use]
    pub const fn new(kind: ToolEffectKind) -> Self {
        Self {
            kind,
            target: String::new(),
            reversibility: kind.default_reversibility(),
            confirmation: kind.default_confirmation(),
            dry_run: DryRunSupport::NotSupported,
        }
    }

    #[must_use]
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    #[must_use]
    pub const fn with_reversibility(mut self, reversibility: EffectReversibility) -> Self {
        self.reversibility = reversibility;
        self
    }

    #[must_use]
    pub const fn with_confirmation(mut self, confirmation: EffectConfirmation) -> Self {
        self.confirmation = confirmation;
        self
    }

    #[must_use]
    pub const fn with_dry_run(mut self, dry_run: DryRunSupport) -> Self {
        self.dry_run = dry_run;
        self
    }

    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        self.kind.is_mutating()
    }

    #[must_use]
    pub fn label(&self) -> String {
        if self.target.is_empty() {
            format!("{:?}", self.kind)
        } else {
            format!("{:?}:{}", self.kind, self.target)
        }
    }
}

impl ToolEffectKind {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        !matches!(
            self,
            Self::ReadFile | Self::ReadSecret | Self::NetworkRequest | Self::IntrospectRuntime
        )
    }

    const fn default_reversibility(self) -> EffectReversibility {
        match self {
            Self::ReadFile | Self::NetworkRequest | Self::IntrospectRuntime => {
                EffectReversibility::Reversible
            }
            Self::WriteFile
            | Self::RunProcess
            | Self::PersistMemory
            | Self::ScheduleTask
            | Self::GenerateMedia
            | Self::DelegateWork => EffectReversibility::PartiallyReversible,
            Self::ReadSecret
            | Self::DeleteFile
            | Self::SendMessage
            | Self::SpendMoney
            | Self::Deploy
            | Self::ModifyCredential
            | Self::PublishContent => EffectReversibility::Irreversible,
        }
    }

    const fn default_confirmation(self) -> EffectConfirmation {
        match self {
            Self::ReadFile | Self::NetworkRequest | Self::IntrospectRuntime => {
                EffectConfirmation::OnRisk
            }
            _ => EffectConfirmation::Always,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Tool emits intermediate progress updates.
    pub emits_progress: bool,
    /// Tool emits observer-lane notes for the parent turn.
    pub emits_observer_text: bool,
    /// Tool is safe to run in background maintenance contexts.
    pub background_safe: bool,
    /// Declarative effect surface used by risk policy and transaction tracing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ToolEffect>,
}

impl ToolCapabilities {
    #[must_use]
    pub fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.effects.push(effect);
        self
    }

    #[must_use]
    pub fn with_effects(mut self, effects: impl IntoIterator<Item = ToolEffect>) -> Self {
        self.effects.extend(effects);
        self
    }
}

/// Runtime bridge presented to tools during execution.
///
/// This allows plugins to consume stable runtime context and emit bounded
/// execution signals without depending on Cortex internals.
pub trait ToolRuntime: Send + Sync {
    /// Stable invocation metadata.
    fn invocation(&self) -> &InvocationContext;

    /// Emit an intermediate progress update for the current tool.
    fn emit_progress(&self, message: &str);

    /// Emit observer text for the parent turn. This never speaks directly to
    /// the user-facing channel.
    fn emit_observer(&self, source: Option<&str>, content: &str);
}
