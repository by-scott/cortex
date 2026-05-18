//! Convenience re-exports for plugin development.
//!
//! ```rust,no_run
//! use cortex_sdk::prelude::*;
//! ```
//!
//! This imports [`MultiToolPlugin`], [`PluginInfo`], [`Tool`],
//! [`ToolError`], [`ToolResult`], and [`serde_json`].

pub use crate::{
    Attachment, CortexBuffer, CortexHostApi, CortexPluginApi, DryRunSupport, EffectConfirmation,
    EffectReversibility, ExecutionScope, InvocationContext, MultiToolPlugin, NATIVE_ABI_VERSION,
    PluginInfo, SDK_VERSION, Tool, ToolCapabilities, ToolEffect, ToolEffectKind, ToolError,
    ToolResult, ToolRuntime,
};
pub use serde_json;
