use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::provenance::{SourceProvenance, SourceTrust};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Ingest,
    Chunk,
    Index,
    QueryPlan,
    Retrieve,
    Rerank,
    Compress,
    Ground,
    Cite,
    Evaluate,
    Promote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Needed,
    Skipped,
    Insufficient,
    Corrected,
    FallbackToHuman,
    FallbackToTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Taint {
    TrustedCorpus,
    UserCorpus,
    ExternalCorpus,
    ToolOutput,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccessClass {
    #[default]
    Public,
    ActorPrivate,
    WorkspacePrivate,
    SystemInternal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryTransformKind {
    Rewrite,
    Expansion,
    HypotheticalDocument,
    Clarification,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub sparse: f32,
    pub dense: f32,
    pub rerank: f32,
    pub graph: f32,
}

impl Default for Scores {
    fn default() -> Self {
        Self {
            sparse: 0.0,
            dense: 0.0,
            rerank: 0.0,
            graph: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub corpus_id: String,
    pub chunk_id: String,
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    pub text: String,
    pub provenance: SourceProvenance,
    pub visibility_actor: String,
    #[serde(default)]
    pub access: AccessClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    pub scores: Scores,
    pub taint: Taint,
    pub retrieved_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlan {
    pub query: String,
    pub actor: String,
    #[serde(default)]
    pub sparse: bool,
    #[serde(default)]
    pub dense: bool,
    #[serde(default)]
    pub graph: bool,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<QueryTransform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryTransform {
    pub kind: QueryTransformKind,
    pub original_query: String,
    pub transformed_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub kind: DecisionKind,
    pub query_plan: QueryPlan,
    pub rationale: String,
    pub support: f32,
    pub decided_at: DateTime<Utc>,
}

impl Scores {
    #[must_use]
    pub const fn best(&self) -> f32 {
        self.sparse
            .max(self.dense)
            .max(self.rerank)
            .max(self.graph)
            .clamp(0.0, 1.0)
    }

    #[must_use]
    pub fn hybrid(&self) -> f32 {
        let weighted = self.graph.mul_add(
            0.10,
            self.rerank
                .mul_add(0.30, self.sparse.mul_add(0.25, self.dense * 0.35)),
        );
        weighted.clamp(0.0, 1.0)
    }
}

impl Evidence {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        corpus_id: impl Into<String>,
        chunk_id: impl Into<String>,
        source_uri: impl Into<String>,
        text: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        let source_uri = source_uri.into();
        Self {
            id: id.into(),
            corpus_id: corpus_id.into(),
            chunk_id: chunk_id.into(),
            source_uri: source_uri.clone(),
            span: None,
            text: text.into(),
            provenance: SourceProvenance::new(source_uri, SourceTrust::Untrusted),
            visibility_actor: actor.into(),
            access: AccessClass::Public,
            license: None,
            source_title: None,
            scores: Scores::default(),
            taint: Taint::ExternalCorpus,
            retrieved_at: Utc::now(),
            index_version: None,
        }
    }

    #[must_use]
    pub const fn with_scores(mut self, scores: Scores) -> Self {
        self.scores = scores;
        self
    }

    #[must_use]
    pub const fn with_taint(mut self, taint: Taint) -> Self {
        self.taint = taint;
        self
    }

    #[must_use]
    pub fn with_span(mut self, span: impl Into<String>) -> Self {
        self.span = Some(span.into());
        self
    }

    #[must_use]
    pub fn with_index_version(mut self, index_version: impl Into<String>) -> Self {
        self.index_version = Some(index_version.into());
        self
    }

    #[must_use]
    pub const fn with_access(mut self, access: AccessClass) -> Self {
        self.access = access;
        self
    }

    #[must_use]
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    #[must_use]
    pub fn with_source_title(mut self, source_title: impl Into<String>) -> Self {
        self.source_title = Some(source_title.into());
        self
    }

    #[must_use]
    pub const fn is_instructional_taint(&self) -> bool {
        !matches!(self.taint, Taint::TrustedCorpus | Taint::UserCorpus)
    }

    #[must_use]
    pub fn citation_key(&self) -> String {
        self.span.as_ref().map_or_else(
            || format!("{}#{}", self.source_uri, self.chunk_id),
            |span| format!("{}#{}:{}", self.source_uri, self.chunk_id, span),
        )
    }
}

impl QueryPlan {
    #[must_use]
    pub fn hybrid(query: impl Into<String>, actor: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            actor: actor.into(),
            sparse: true,
            dense: true,
            graph: false,
            filters: Vec::new(),
            transforms: Vec::new(),
        }
    }

    #[must_use]
    pub const fn with_graph(mut self) -> Self {
        self.graph = true;
        self
    }

    #[must_use]
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filters.push(filter.into());
        self
    }

    #[must_use]
    pub fn with_transform(mut self, transform: QueryTransform) -> Self {
        self.transforms.push(transform);
        self
    }

    #[must_use]
    pub fn dense_query_text(&self) -> String {
        self.transforms
            .iter()
            .rev()
            .find_map(|transform| transform.generated_text.clone())
            .unwrap_or_else(|| self.query.clone())
    }
}

impl Decision {
    #[must_use]
    pub fn new(kind: DecisionKind, query_plan: QueryPlan, rationale: impl Into<String>) -> Self {
        Self {
            kind,
            query_plan,
            rationale: rationale.into(),
            support: 0.0,
            decided_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn with_support(mut self, support: f32) -> Self {
        self.support = support.clamp(0.0, 1.0);
        self
    }
}

impl QueryTransform {
    #[must_use]
    pub fn new(
        kind: QueryTransformKind,
        original_query: impl Into<String>,
        transformed_query: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            original_query: original_query.into(),
            transformed_query: transformed_query.into(),
            generated_text: None,
            created_at: Utc::now(),
        }
    }

    #[must_use]
    pub fn hypothetical_document(
        original_query: impl Into<String>,
        generated_text: impl Into<String>,
    ) -> Self {
        let original_query = original_query.into();
        Self {
            kind: QueryTransformKind::HypotheticalDocument,
            original_query: original_query.clone(),
            transformed_query: original_query,
            generated_text: Some(generated_text.into()),
            created_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn is_evidence(&self) -> bool {
        false
    }
}
