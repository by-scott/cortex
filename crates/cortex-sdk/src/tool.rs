use serde::{Deserialize, Serialize};

use crate::{Attachment, ToolCapabilities, ToolRuntime};

/// A tool that the LLM can invoke during conversation.
///
/// Tools are the primary extension point for Cortex plugins.  Each tool
/// has a name, description, JSON Schema for input parameters, and an
/// execute function.  The runtime presents the tool definition to the LLM
/// and routes invocations to [`Tool::execute`].
///
/// # Thread Safety
///
/// Tools must be `Send + Sync` because a single tool instance is shared
/// across all turns in the daemon process.  Use interior mutability
/// (`Mutex`, `RwLock`, `AtomicXxx`) if you need mutable state.
pub trait Tool: Send + Sync {
    /// Unique tool name (lowercase, underscores, e.g. `"web_search"`).
    ///
    /// Must be unique across all registered tools.  If two tools share a
    /// name, the later registration wins.
    fn name(&self) -> &'static str;

    /// Human-readable description shown to the LLM.
    ///
    /// Write this for the LLM, not for humans.  Include:
    /// - What the tool does
    /// - When to use it
    /// - When *not* to use it
    /// - Any constraints or limitations
    fn description(&self) -> &'static str;

    /// JSON Schema describing the tool's input parameters.
    ///
    /// The LLM generates a JSON object matching this schema.  Example:
    ///
    /// ```json
    /// {
    ///   "type": "object",
    ///   "properties": {
    ///     "query": { "type": "string", "description": "Search query" }
    ///   },
    ///   "required": ["query"]
    /// }
    /// ```
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given input.
    ///
    /// `input` is a JSON object matching [`Self::input_schema`].  The
    /// runtime validates the schema before calling this method, but
    /// individual field types should still be checked defensively.
    ///
    /// # Return Values
    ///
    /// - [`ToolResult::success`] — normal output returned to the LLM
    /// - [`ToolResult::error`] — the tool ran but produced an error the
    ///   LLM should see and potentially recover from
    ///
    /// # Errors
    ///
    /// Return [`ToolError::InvalidInput`] for malformed parameters or
    /// [`ToolError::ExecutionFailed`] for unrecoverable failures.  These
    /// are surfaced as error events in the turn journal.
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError>;

    /// Execute the tool with runtime context and host callbacks.
    ///
    /// Plugins can override this to read session/actor/source metadata and
    /// emit progress or observer updates through the provided runtime bridge.
    ///
    /// The default implementation preserves the classic SDK contract and calls
    /// [`Self::execute`].
    ///
    /// # Errors
    ///
    /// Returns the same `ToolError` variants that [`Self::execute`] would
    /// return for invalid input or unrecoverable execution failure.
    fn execute_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &dyn ToolRuntime,
    ) -> Result<ToolResult, ToolError> {
        let _ = runtime;
        self.execute(input)
    }

    /// Optional per-tool execution timeout in seconds.
    ///
    /// If `None` (the default), the global `[turn].tool_timeout_secs`
    /// from the instance configuration applies.
    fn timeout_secs(&self) -> Option<u64> {
        None
    }

    /// Stable capability hints consumed by the runtime and observability
    /// layers.
    fn capabilities(&self) -> ToolCapabilities {
        ToolCapabilities::default()
    }
}

/// Result of a tool execution returned to the LLM.
///
/// Use [`ToolResult::success`] for normal output and [`ToolResult::error`]
/// for recoverable errors the LLM should see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Output text returned to the LLM.
    pub output: String,
    /// Structured media attachments produced by this tool.
    ///
    /// Attachments are delivered by Cortex transports independently from the
    /// text the model sees, so tools do not need transport-specific protocols.
    pub media: Vec<Attachment>,
    /// Whether this result represents an error condition.
    ///
    /// When `true`, the LLM sees this as a failed tool call and may retry
    /// with different parameters or switch strategy.
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful result.
    #[must_use]
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            media: Vec::new(),
            is_error: false,
        }
    }

    /// Create an error result (tool ran but failed).
    ///
    /// Use this for recoverable errors; the LLM sees the output and can
    /// decide how to proceed. For example: "file not found", "permission
    /// denied", "rate limit exceeded".
    #[must_use]
    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            media: Vec::new(),
            is_error: true,
        }
    }

    /// Attach one media item to the result.
    #[must_use]
    pub fn with_media(mut self, attachment: Attachment) -> Self {
        self.media.push(attachment);
        self
    }

    /// Attach multiple media items to the result.
    #[must_use]
    pub fn with_media_many(mut self, media: impl IntoIterator<Item = Attachment>) -> Self {
        self.media.extend(media);
        self
    }
}

/// Error from tool execution.
///
/// Unlike [`ToolResult::error`] (which is a "soft" error the LLM sees),
/// `ToolError` represents a hard failure that is logged in the turn
/// journal as a tool invocation error.
#[derive(Debug)]
pub enum ToolError {
    /// Input parameters are invalid or missing required fields.
    InvalidInput(String),
    /// Execution failed due to an external or internal error.
    ExecutionFailed(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(e) => write!(f, "invalid input: {e}"),
            Self::ExecutionFailed(e) => write!(f, "execution failed: {e}"),
        }
    }
}

impl std::error::Error for ToolError {}

/// Plugin metadata returned to the runtime at load time.
///
/// The `name` field must match the plugin's directory name and the
/// `name` field in `manifest.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Unique plugin identifier (e.g. `"my-plugin"`).
    pub name: String,
    /// Plugin semantic version string (e.g. `"0.1.0"`).
    pub version: String,
    /// Human-readable one-line description.
    pub description: String,
}

/// A plugin that provides multiple tools from a single shared library.
///
/// This is the primary interface between a plugin and the Cortex runtime.
/// Implement this trait and use [`export_plugin!`] to generate the FFI
/// entry point.
///
/// # Requirements
///
/// - The implementing type must also implement `Default` (required by
///   [`export_plugin!`] for construction via FFI).
/// - The type must be `Send + Sync` because the runtime may access it
///   from multiple threads.
///
/// # Example
///
/// ```rust,no_run
/// use cortex_sdk::prelude::*;
///
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl MultiToolPlugin for MyPlugin {
///     fn plugin_info(&self) -> PluginInfo {
///         PluginInfo {
///             name: "my-plugin".into(),
///             version: "0.1.0".into(),
///             description: "Example plugin".into(),
///         }
///     }
///
///     fn create_tools(&self) -> Vec<Box<dyn Tool>> {
///         vec![]
///     }
/// }
///
/// cortex_sdk::export_plugin!(MyPlugin);
/// ```
pub trait MultiToolPlugin: Send + Sync {
    /// Return plugin metadata.
    fn plugin_info(&self) -> PluginInfo;

    /// Create all tools this plugin provides.
    ///
    /// Called once at daemon startup.  Returned tools live for the
    /// daemon's lifetime.  Each tool is registered by name into the
    /// global tool registry.
    fn create_tools(&self) -> Vec<Box<dyn Tool>>;
}
