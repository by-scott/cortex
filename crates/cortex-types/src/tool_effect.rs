use serde::{Deserialize, Serialize};

use crate::RiskLevel;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectReversibility {
    Reversible,
    PartiallyReversible,
    Irreversible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectConfirmation {
    Never,
    OnRisk,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DryRunSupport {
    NotSupported,
    Supported,
    RequiredBeforeExecute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEffect {
    pub kind: ToolEffectKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    pub reversibility: EffectReversibility,
    pub confirmation: EffectConfirmation,
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
    pub const fn risk_floor(&self) -> RiskLevel {
        let mut floor = self.kind.risk_floor();
        if matches!(self.confirmation, EffectConfirmation::Always)
            && matches!(floor, RiskLevel::Allow | RiskLevel::Review)
        {
            floor = RiskLevel::RequireConfirmation;
        }
        if matches!(self.reversibility, EffectReversibility::Irreversible)
            && matches!(floor, RiskLevel::Allow | RiskLevel::Review)
        {
            floor = RiskLevel::RequireConfirmation;
        }
        floor
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

    #[must_use]
    pub const fn risk_floor(self) -> RiskLevel {
        match self {
            Self::ReadFile | Self::IntrospectRuntime => RiskLevel::Allow,
            Self::NetworkRequest | Self::PersistMemory | Self::GenerateMedia => RiskLevel::Review,
            Self::WriteFile
            | Self::RunProcess
            | Self::SendMessage
            | Self::PublishContent
            | Self::ScheduleTask
            | Self::DelegateWork => RiskLevel::RequireConfirmation,
            Self::ReadSecret
            | Self::DeleteFile
            | Self::SpendMoney
            | Self::Deploy
            | Self::ModifyCredential => RiskLevel::Block,
        }
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
