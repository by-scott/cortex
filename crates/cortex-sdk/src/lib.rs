//! # Cortex SDK
//!
//! The official SDK for building [Cortex](https://github.com/by-scott/cortex)
//! plugins.
//!
//! This crate defines the public plugin surface with **zero dependency on
//! Cortex internals**. The runtime loads plugins via FFI and bridges these
//! traits to its own turn runtime, command surface, and transport layer.
//!
//! ## Architecture
//!
//! ```text
//!  ┌──────────────┐     dlopen      ┌──────────────────┐
//!  │ cortex-runtime│ ──────────────▶ │  your plugin.so  │
//!  │   (daemon)    │                 │  cortex-sdk only  │
//!  └──────┬───────┘   FFI call      └────────┬─────────┘
//!         │        cortex_plugin_              │
//!         │        create_multi()              │
//!         ▼                                    ▼
//!    ToolRegistry  ◀─── register ───  MultiToolPlugin
//!                                     ├─ plugin_info()
//!                                     └─ create_tools()
//!                                         ├─ Tool A
//!                                         └─ Tool B
//! ```
//!
//! Plugins are compiled as `cdylib` shared libraries. The runtime calls a
//! single FFI entry point (`cortex_plugin_create_multi`) that returns a
//! [`MultiToolPlugin`] trait object. Each tool returned by
//! [`MultiToolPlugin::create_tools`] is registered into the global tool
//! registry and becomes available to the LLM during turns.
//!
//! The SDK now exposes a runtime-aware execution surface as well:
//!
//! - [`InvocationContext`] gives tools stable metadata such as session id,
//!   canonical actor, transport/source, and foreground/background scope
//! - [`ToolRuntime`] lets tools emit progress updates and observer text back
//!   to the parent turn
//! - [`ToolCapabilities`] lets tools declare whether they emit runtime signals
//!   and whether they are background-safe
//! - [`Attachment`] and [`ToolResult::with_media`] let tools return structured
//!   image, audio, video, or file outputs without depending on Cortex internals
//! ## Quick Start
//!
//! **Cargo.toml:**
//!
//! ```toml
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! cortex-sdk = "1.2"
//! serde_json = "1"
//! ```
//!
//! **src/lib.rs:**
//!
//! ```rust,no_run
//! use cortex_sdk::prelude::*;
//!
//! // 1. Define the plugin entry point.
//! #[derive(Default)]
//! struct MyPlugin;
//!
//! impl MultiToolPlugin for MyPlugin {
//!     fn plugin_info(&self) -> PluginInfo {
//!         PluginInfo {
//!             name: "my-plugin".into(),
//!             version: env!("CARGO_PKG_VERSION").into(),
//!             description: "My custom tools for Cortex".into(),
//!         }
//!     }
//!
//!     fn create_tools(&self) -> Vec<Box<dyn Tool>> {
//!         vec![Box::new(WordCountTool)]
//!     }
//! }
//!
//! // 2. Implement one or more tools.
//! struct WordCountTool;
//!
//! impl Tool for WordCountTool {
//!     fn name(&self) -> &'static str { "word_count" }
//!
//!     fn description(&self) -> &'static str {
//!         "Count words in a text string. Use when the user asks for word \
//!          counts, statistics, or text length metrics."
//!     }
//!
//!     fn input_schema(&self) -> serde_json::Value {
//!         serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "text": {
//!                     "type": "string",
//!                     "description": "The text to count words in"
//!                 }
//!             },
//!             "required": ["text"]
//!         })
//!     }
//!
//!     fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
//!         let text = input["text"]
//!             .as_str()
//!             .ok_or_else(|| ToolError::InvalidInput("missing 'text' field".into()))?;
//!         let count = text.split_whitespace().count();
//!         Ok(ToolResult::success(format!("{count} words")))
//!     }
//! }
//!
//! // 3. Export the FFI entry point.
//! cortex_sdk::export_plugin!(MyPlugin);
//! ```
//!
//! Tools that need runtime context can override
//! [`Tool::execute_with_runtime`] instead of only [`Tool::execute`].
//!
//! ## Build & Install
//!
//! ```bash
//! cargo build --release
//! mkdir -p my-plugin/lib
//! cp target/release/libmy_plugin.so my-plugin/lib/     # .dylib on macOS
//! cortex plugin install ./my-plugin/
//! ```
//!
//! ## Plugin Lifecycle
//!
//! 1. **Load** — `dlopen` at daemon startup
//! 2. **Create** — runtime calls [`export_plugin!`]-generated FFI function
//! 3. **Register** — [`MultiToolPlugin::create_tools`] is called once; each
//!    [`Tool`] is registered in the global tool registry
//! 4. **Execute** — the LLM invokes tools by name during turns; the runtime
//!    calls [`Tool::execute`] with JSON parameters
//! 5. **Retain** — the library handle is held for the daemon's lifetime;
//!    `Drop` runs only at shutdown
//!
//! ## Tool Design Guidelines
//!
//! - **`name`**: lowercase with underscores (`word_count`, not `WordCount`).
//!   Must be unique across all tools in the registry.
//! - **`description`**: written for the LLM — explain what the tool does,
//!   when to use it, and when *not* to use it.  The LLM reads this to decide
//!   whether to call the tool.
//! - **`input_schema`**: a [JSON Schema](https://json-schema.org/) object
//!   describing the parameters.  The LLM generates JSON matching this schema.
//! - **`execute`**: receives the LLM-generated JSON.  Return
//!   [`ToolResult::success`] for normal output or [`ToolResult::error`] for
//!   recoverable errors the LLM should see.  Return [`ToolError`] only for
//!   unrecoverable failures (invalid input, missing deps).
//! - **Media output**: attach files with [`ToolResult::with_media`].  Cortex
//!   delivers attachments through the active transport; plugins should not call
//!   channel-specific APIs directly.
//! - **`execute_with_runtime`**: use this when the tool needs invocation
//!   metadata or wants to emit progress / observer updates during execution.
//! - **`timeout_secs`**: optional per-tool timeout override.  If `None`, the
//!   global `[turn].tool_timeout_secs` applies.

pub use serde_json;

/// Version of the SDK crate used by native plugin compatibility checks.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ABI revision for in-process native plugins.
///
/// Cortex's current in-process plugin path exchanges Rust trait objects over
/// an FFI-loaded shared library boundary. This revision is bumped whenever that
/// boundary changes in a way that requires rebuilding native plugins.
pub const ABI_REVISION: u32 = 2;

/// Stable multimedia attachment DTO exposed to plugins.
///
/// This type intentionally lives in `cortex-sdk` instead of depending on
/// Cortex internal crates, so plugin authors only need the SDK.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// High-level type: `"image"`, `"audio"`, `"video"`, `"file"`.
    pub media_type: String,
    /// MIME type, for example `"image/png"` or `"audio/mpeg"`.
    pub mime_type: String,
    /// Local file path or remote URL readable by the runtime transport.
    pub url: String,
    /// Optional caption or description.
    pub caption: Option<String>,
    /// File size in bytes, if known.
    pub size: Option<u64>,
}

/// Whether a tool invocation belongs to a user-visible foreground turn or a
/// background maintenance execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionScope {
    #[default]
    Foreground,
    Background,
}

/// Stable runtime metadata exposed to plugin tools during execution.
///
/// This intentionally exposes the execution surface, not Cortex internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationContext {
    /// Tool name being invoked.
    pub tool_name: String,
    /// Active session id when available.
    pub session_id: Option<String>,
    /// Canonical actor identity when available.
    pub actor: Option<String>,
    /// Transport or invocation source (`http`, `rpc`, `telegram`, `heartbeat`, ...).
    pub source: Option<String>,
    /// Whether this invocation belongs to a foreground or background execution.
    pub execution_scope: ExecutionScope,
}

impl InvocationContext {
    #[must_use]
    pub fn is_background(&self) -> bool {
        self.execution_scope == ExecutionScope::Background
    }

    #[must_use]
    pub fn is_foreground(&self) -> bool {
        self.execution_scope == ExecutionScope::Foreground
    }
}

/// Declarative hints about how a tool participates in the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolCapabilities {
    /// Tool emits intermediate progress updates.
    pub emits_progress: bool,
    /// Tool emits observer-lane notes for the parent turn.
    pub emits_observer_text: bool,
    /// Tool is safe to run in background maintenance contexts.
    pub background_safe: bool,
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

// ── Tool Interface ──────────────────────────────────────────

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
#[derive(Debug, Clone)]
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
    /// Use this for recoverable errors — the LLM sees the output and can
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

// ── Plugin Interface ────────────────────────────────────────

/// Plugin metadata returned to the runtime at load time.
///
/// The `name` field must match the plugin's directory name and the
/// `name` field in `manifest.toml`.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Unique plugin identifier (e.g. `"my-plugin"`).
    pub name: String,
    /// Semantic version string (e.g. `"1.2.0"`).
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

// ── Export Macro ────────────────────────────────────────────

/// Generate the FFI entry point for a [`MultiToolPlugin`].
///
/// This macro expands to an `extern "C"` function named
/// `cortex_plugin_create_multi` that the runtime calls via `dlopen` /
/// `dlsym`.  The plugin type must implement [`Default`].
///
/// # Usage
///
/// `cortex_sdk::export_plugin!(MyPlugin);`
///
/// # Expansion
///
/// The macro expands to an `extern "C" fn cortex_plugin_create_multi()`
/// that constructs the plugin via `Default::default()` and returns a raw
/// pointer to the `MultiToolPlugin` trait object.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn cortex_plugin_create_multi() -> *mut dyn $crate::MultiToolPlugin {
            Box::into_raw(Box::new(<$plugin_type>::default()))
        }
    };
}

// ── Prelude ─────────────────────────────────────────────────

/// Convenience re-exports for plugin development.
///
/// ```rust,no_run
/// use cortex_sdk::prelude::*;
/// ```
///
/// This imports [`MultiToolPlugin`], [`PluginInfo`], [`Tool`],
/// [`ToolError`], [`ToolResult`], and [`serde_json`].
pub mod prelude {
    pub use super::{
        Attachment, ExecutionScope, InvocationContext, MultiToolPlugin, PluginInfo, Tool,
        ToolCapabilities, ToolError, ToolResult, ToolRuntime,
    };
    pub use serde_json;
}
