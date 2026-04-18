#![warn(clippy::pedantic, clippy::nursery)]

// Storage
pub mod audit;
pub mod db_writer;
pub mod goal_store;
pub mod journal;
pub mod memory_graph;
pub mod memory_store;
pub mod session_store;
pub mod task_store;

// Config
pub mod config_loader;
pub mod config_validator;
pub mod config_watcher;

// Embedding
pub mod embedding_client;
pub mod embedding_evaluator;
pub mod embedding_store;

// Model info & vision
pub mod model_info;
pub mod vision_discovery;

// Prompt
pub mod prompt_manager;

// Replay
pub mod replay;

// Internal
mod util;

// Re-exports: storage
pub use audit::{AuditEntry, AuditError, AuditEventType, AuditLog};
pub use db_writer::DbWriter;
pub use goal_store::GoalStore;
pub use journal::{Journal, JournalError, StoredEvent};
pub use memory_graph::{MemoryGraph, MemoryGraphError};
pub use memory_store::MemoryStore;
pub use session_store::SessionStore;
pub use task_store::{TaskStore, TaskStoreError};

// Re-exports: config
pub use config_loader::{
    ensure_base_dirs, ensure_home_dirs, format_config_section, format_config_summary, load_config,
    load_providers, resolve_home,
};
pub use config_validator::{config_health, validate};
pub use config_watcher::{ConfigWatcher, ConfigWatcherError};

// Re-exports: embedding
pub use embedding_client::{EmbeddingClient, EmbeddingError};
pub use embedding_evaluator::EmbeddingEvaluator;
pub use embedding_store::EmbeddingStore;

// Re-exports: model & vision
pub use model_info::{ModelInfo, ModelInfoStore};
pub use vision_discovery::VisionCapStore;

// Re-exports: prompt
pub use prompt_manager::PromptManager;

// Re-exports: replay
pub use replay::{
    JournalSideEffectProvider, SideEffectProvider, TurnSummary, project_message_history,
    project_turn_summaries, replay, replay_with_sideeffects,
};
