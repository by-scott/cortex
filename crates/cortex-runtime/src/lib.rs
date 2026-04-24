#![warn(clippy::pedantic, clippy::nursery)]

pub mod channels;
pub mod client;
pub mod command_registry;
pub mod daemon;
pub mod heartbeat;
pub mod hot_reload;
pub mod instance_manager;
pub mod introspect_tools;
pub mod media;
pub mod metrics;
pub mod plugin_loader;
pub mod rate_limiter;
pub mod rpc;
pub mod runtime;
pub mod session_manager;
pub mod stability;
pub mod tls;
pub mod turn_executor;

#[cfg(test)]
mod tests;

pub use client::{ClientError, DaemonClient, StreamEvent};
pub use command_registry::{
    CommandContext, CommandRegistry, CommandResult, CommandScope, DefaultCommandRegistry,
    ParsedCommand,
};
pub use daemon::{BroadcastEvent, BroadcastMessage, DaemonConfig, DaemonServer};
pub use instance_manager::{InstanceInfo, InstanceManager, InstanceNotFound};
pub use rpc::{RpcError, RpcHandler, RpcRequest, RpcResponse};
pub use runtime::{CortexRuntime, RuntimeError};
pub use session_manager::{ResumedSession, SessionError, SessionManager};
pub use stability::{
    ResourceSnapshot, StabilityMonitor, StabilityReport, TrendResult, detect_trend,
};
pub use turn_executor::{TurnCallbacks, TurnExecutor, TurnExecutorConfig, TurnOutput};

// Re-exports from cortex-turn for downstream crates (avoids layer violation).
pub use cortex_turn::context::SummaryCache;
pub use cortex_turn::meta::MetaMonitor;
pub use cortex_turn::orchestrator::resume::build_resume_packet;
pub use cortex_turn::orchestrator::{ToolProgress, ToolProgressStatus};
pub use cortex_turn::plugin::{PluginInfo, PluginRegistry};
pub use cortex_turn::risk::PermissionGate;
pub use cortex_turn::tools::ToolRegistry;
