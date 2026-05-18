//! # Cortex SDK
//!
//! The official Rust SDK for Cortex's trusted native plugin boundary.
//!
//! This crate defines the public plugin surface with **zero dependency on
//! Cortex internals**. The runtime loads trusted native plugins through a
//! stable C-compatible ABI and bridges these traits to its own turn runtime,
//! command surface, and transport layer.
//!
//! Process-isolated JSON plugins do **not** need this crate. They are defined
//! through `manifest.toml` plus a child-process command. Use `cortex-sdk` when
//! you are building a trusted in-process native plugin that exports
//! `cortex_plugin_init`.
//!
//! SDK release cadence is independent from Cortex runtime releases. Choose the
//! SDK version by the native ABI/DTO surface your plugin needs, not by the
//! latest Cortex runtime patch. Runtime compatibility is declared in the
//! plugin `manifest.toml` `cortex_version` field, which is the minimum Cortex
//! runtime version the plugin supports.
//!
//! ## Architecture
//!
//! ```text
//!  ┌──────────────┐     dlopen      ┌──────────────────┐
//!  │ cortex-runtime│ ──────────────▶ │  your plugin.so  │
//!  │   (daemon)    │                 │  cortex-sdk only  │
//!  └──────┬───────┘   FFI call      └────────┬─────────┘
//!         │        cortex_plugin_init()         │
//!         ▼                                    ▼
//!    ToolRegistry  ◀─── register ───  MultiToolPlugin
//!                                     ├─ plugin_info()
//!                                     └─ create_tools()
//!                                         ├─ Tool A
//!                                         └─ Tool B
//! ```
//!
//! Plugins are compiled as `cdylib` shared libraries. The runtime calls
//! `cortex_plugin_init`, receives a C-compatible function table, then asks that
//! table for plugin metadata, tool descriptors, and tool execution results.
//! Rust trait objects stay inside the plugin; they never cross the
//! dynamic-library boundary.
//!
//! The SDK exposes a runtime-aware execution surface:
//!
//! - [`InvocationContext`] gives tools stable metadata such as session id,
//!   canonical actor, transport/source, and foreground/background scope
//! - [`ToolRuntime`] lets tools emit progress updates and observer text back
//!   to the parent turn
//! - [`ToolCapabilities`] lets tools declare whether they emit runtime signals
//!   and whether they are background-safe
//! - [`Attachment`] and [`ToolResult::with_media`] let tools return structured
//!   image, audio, video, or file outputs without depending on Cortex internals
//! ## Native Plugin Quick Start
//!
//! **Cargo.toml:**
//!
//! ```toml
//! [package]
//! name = "cortex-plugin-native-hello"
//! version = "0.1.0"
//! edition = "2024"
//! publish = false
//!
//! [lib]
//! crate-type = ["cdylib", "rlib"]
//!
//! [dependencies]
//! cortex-sdk = "1.6.4"
//! serde_json = "1"
//! ```
//!
//! **src/lib.rs:**
//!
//! ```rust,no_run
//! use cortex_sdk::prelude::*;
//!
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
//! cortex_sdk::export_plugin!(MyPlugin);
//! ```
//!
//! Tools that need runtime context can override
//! [`Tool::execute_with_runtime`] instead of only [`Tool::execute`].
//!
//! **manifest.toml:**
//!
//! ```toml
//! name = "native-hello"
//! version = "0.1.0"
//! description = "Example trusted native Cortex plugin"
//! cortex_version = "1.6.0"
//! trust = "trusted_native"
//!
//! [capabilities]
//! provides = ["tools"]
//! secrets = false
//!
//! [sandbox]
//! level = "trusted_in_process"
//!
//! [native]
//! library = "lib/libcortex_plugin_native_hello.so"
//! isolation = "trusted_in_process"
//! abi_version = 1
//! ```
//!
//! ## Build, Sign, Pack, Publish
//!
//! ```bash
//! cargo build --release
//! cortex plugin review .
//! cortex plugin test .
//! cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
//! cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
//! cortex plugin pack .
//! sha256sum cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx.sha256
//! ```
//!
//! Upload the `.cpx` and `.sha256` files to a GitHub Release. Users install the
//! package by repository name and restart the daemon so the native library is
//! loaded:
//!
//! ```bash
//! cortex plugin install owner/cortex-plugin-native-hello@0.1.0
//! cortex restart
//! ```
//!
//! Installing or replacing a trusted native shared library still requires a
//! daemon restart so the new code is loaded. Process-isolated plugin manifest
//! changes hot-apply without that restart.
//!
//! ## Plugin Lifecycle
//!
//! 1. **Load** — `dlopen` at daemon startup
//! 2. **Create** — runtime calls [`export_plugin!`]-generated stable ABI init
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

use serde::{Deserialize, Serialize};
pub use serde_json;

mod native;
pub mod prelude;

pub use native::{
    CortexBuffer, CortexHostApi, CortexPluginApi, NativePluginState, cortex_buffer_free,
    native_plugin_drop, native_plugin_info, native_tool_count, native_tool_descriptor,
    native_tool_execute,
};

/// Version of the SDK crate used by native plugin builds.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable native ABI version for trusted in-process plugins.
///
/// The runtime never exchanges Rust trait objects across the dynamic-library
/// boundary. It loads a C-compatible function table through `cortex_plugin_init`
/// and moves structured values as UTF-8 JSON buffers.
pub const NATIVE_ABI_VERSION: u32 = 1;

/// Stable multimedia attachment DTO exposed to plugins.
///
/// This type intentionally lives in `cortex-sdk` instead of depending on
/// Cortex internal crates, so plugin authors only need the SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    #[default]
    Foreground,
    Background,
}

/// Stable runtime metadata exposed to plugin tools during execution.
///
/// This intentionally exposes the execution surface, not Cortex internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// ── Export Macro ────────────────────────────────────────────

/// Generate the stable native ABI entry point for a [`MultiToolPlugin`].
///
/// This macro expands to an `extern "C"` function named `cortex_plugin_init`
/// that fills a C-compatible function table. The plugin type must implement
/// [`Default`].
///
/// # Usage
///
/// `cortex_sdk::export_plugin!(MyPlugin);`
///
/// # Expansion
///
/// The macro constructs the Rust plugin internally and exposes it through the
/// stable native ABI table. Rust trait objects never cross the dynamic-library
/// boundary.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn cortex_plugin_init(
            host: *const $crate::CortexHostApi,
            out_plugin: *mut $crate::CortexPluginApi,
        ) -> i32 {
            if host.is_null() || out_plugin.is_null() {
                return -1;
            }
            let host = unsafe { &*host };
            if host.abi_version != $crate::NATIVE_ABI_VERSION {
                return -2;
            }
            let plugin: Box<dyn $crate::MultiToolPlugin> = Box::new(<$plugin_type>::default());
            let state = Box::new($crate::NativePluginState::new(plugin));
            unsafe {
                *out_plugin = $crate::CortexPluginApi {
                    abi_version: $crate::NATIVE_ABI_VERSION,
                    plugin: Box::into_raw(state).cast(),
                    plugin_info: Some($crate::native_plugin_info),
                    tool_count: Some($crate::native_tool_count),
                    tool_descriptor: Some($crate::native_tool_descriptor),
                    tool_execute: Some($crate::native_tool_execute),
                    plugin_drop: Some($crate::native_plugin_drop),
                    buffer_free: Some($crate::cortex_buffer_free),
                };
            }
            0
        }
    };
}
