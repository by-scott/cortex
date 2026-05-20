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
//! cortex-sdk = "1.6.11"
//! serde_json = "1"
//! ```
//!
//! **src/lib.rs:**
//!
//! ```text
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
//! cortex_version = "1.6.11"
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
//! 2. **Create** — runtime calls `export_plugin!`-generated stable ABI init
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

mod effects;
mod native;
pub mod prelude;
mod tool;

pub use effects::{
    DryRunSupport, EffectConfirmation, EffectReversibility, ToolCapabilities, ToolEffect,
    ToolEffectKind, ToolRuntime,
};
pub use native::{
    CortexBuffer, CortexHostApi, CortexPluginApi, NativePluginState, cortex_buffer_free,
    native_plugin_drop, native_plugin_info, native_tool_count, native_tool_descriptor,
    native_tool_execute,
};
pub use tool::{MultiToolPlugin, PluginInfo, Tool, ToolError, ToolResult};

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
