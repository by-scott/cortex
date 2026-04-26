use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AuthContext, OwnedScope, Visibility};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceItemKind {
    UserInput,
    RuntimePolicy,
    Goal,
    RetrievalEvidence,
    Memory,
    ToolSchema,
    ToolResult,
    DeliveryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBudget {
    pub max_items: usize,
    pub max_tokens: usize,
}

impl Default for WorkspaceBudget {
    fn default() -> Self {
        Self {
            max_items: 32,
            max_tokens: 16_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceItem {
    pub id: String,
    pub scope: OwnedScope,
    pub kind: WorkspaceItemKind,
    pub content: String,
    pub token_estimate: usize,
    pub salience: f32,
    pub urgency: f32,
    pub admitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedItem {
    pub id: String,
    pub reason: String,
    pub priority: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscriber {
    pub name: String,
    pub scope: OwnedScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BroadcastFrame {
    pub scope: OwnedScope,
    pub budget: WorkspaceBudget,
    pub items: Vec<WorkspaceItem>,
    pub dropped: Vec<DroppedItem>,
    pub subscribers: Vec<Subscriber>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    NotVisible,
    TooLarge { max_tokens: usize },
}

impl WorkspaceItem {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: OwnedScope,
        kind: WorkspaceItemKind,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            kind,
            content: content.into(),
            token_estimate: 0,
            salience: 0.0,
            urgency: 0.0,
            admitted_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn with_scores(mut self, salience: f32, urgency: f32) -> Self {
        self.salience = salience.clamp(0.0, 1.0);
        self.urgency = urgency.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_tokens(mut self, token_estimate: usize) -> Self {
        self.token_estimate = token_estimate;
        self
    }

    #[must_use]
    pub fn priority(&self) -> f32 {
        self.salience.mul_add(0.65, self.urgency * 0.35)
    }
}

impl BroadcastFrame {
    #[must_use]
    pub fn new(scope: OwnedScope, budget: WorkspaceBudget) -> Self {
        Self {
            scope,
            budget,
            items: Vec::new(),
            dropped: Vec::new(),
            subscribers: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// # Errors
    /// Returns an error when the item is not visible to the frame owner or is
    /// larger than the whole frame token budget.
    pub fn admit(&mut self, item: WorkspaceItem) -> Result<(), AdmissionError> {
        let context = AuthContext::new(
            self.scope.tenant_id.clone(),
            self.scope.actor_id.clone(),
            self.scope.client_id.clone().unwrap_or_default(),
        );
        if !item.scope.is_visible_to(&context) {
            return Err(AdmissionError::NotVisible);
        }
        if item.token_estimate > self.budget.max_tokens {
            return Err(AdmissionError::TooLarge {
                max_tokens: self.budget.max_tokens,
            });
        }

        self.items.push(item);
        self.items.sort_by(|left, right| {
            right
                .priority()
                .total_cmp(&left.priority())
                .then_with(|| left.id.cmp(&right.id))
        });
        self.enforce_budget();
        Ok(())
    }

    pub fn subscribe(&mut self, name: impl Into<String>, context: &AuthContext) {
        let scope = OwnedScope::new(
            context.tenant_id.clone(),
            context.actor_id.clone(),
            Some(context.client_id.clone()),
            Visibility::ActorShared,
        );
        self.subscribers.push(Subscriber {
            name: name.into(),
            scope,
        });
    }

    #[must_use]
    pub fn visible_subscribers(&self, item: &WorkspaceItem) -> Vec<String> {
        self.subscribers
            .iter()
            .filter(|subscriber| {
                let client_id = subscriber
                    .scope
                    .client_id
                    .clone()
                    .unwrap_or_else(crate::ClientId::new);
                let context = AuthContext::new(
                    subscriber.scope.tenant_id.clone(),
                    subscriber.scope.actor_id.clone(),
                    client_id,
                );
                item.scope.is_visible_to(&context)
            })
            .map(|subscriber| subscriber.name.clone())
            .collect()
    }

    fn enforce_budget(&mut self) {
        while self.items.len() > self.budget.max_items
            || self.total_tokens() > self.budget.max_tokens
        {
            let Some(item) = self.items.pop() else {
                break;
            };
            let priority = item.priority();
            self.dropped.push(DroppedItem {
                id: item.id,
                reason: "workspace_budget".to_string(),
                priority,
            });
        }
    }

    #[must_use]
    pub fn total_tokens(&self) -> usize {
        self.items.iter().map(|item| item.token_estimate).sum()
    }
}
