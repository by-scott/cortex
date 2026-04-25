pub mod agent;
pub mod bash;
pub mod cron;
pub mod edit;
pub mod image_gen;
pub mod memory_tools;
pub mod read;
pub mod send_media;
pub mod tts;
pub mod video_gen;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// Tool interface defined in cortex-sdk — re-exported here for internal use.
use cortex_sdk::Attachment;
pub use cortex_sdk::{Tool, ToolError, ToolResult};

pub(crate) fn attachment_from_path(media_type: &str, mime_type: &str, path: &str) -> Attachment {
    let size = std::fs::metadata(path).ok().map(|m| m.len());
    Attachment {
        media_type: media_type.to_string(),
        mime_type: mime_type.to_string(),
        url: path.to_string(),
        caption: None,
        size,
    }
}

pub(crate) fn infer_media_type(mime_type: &str, file_name: Option<&str>) -> &'static str {
    let mime = mime_type.to_ascii_lowercase();
    if mime.starts_with("image/") {
        return "image";
    }
    if mime.starts_with("audio/") {
        return "audio";
    }
    if mime.starts_with("video/") {
        return "video";
    }
    if let Some(name) = file_name {
        let lower = name.to_ascii_lowercase();
        if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "image";
        }
        if [".ogg", ".mp3", ".wav", ".m4a", ".aac", ".opus"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "audio";
        }
        if [".mp4", ".mov", ".mkv", ".webm", ".avi"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "video";
        }
    }
    "file"
}

pub(crate) fn infer_mime_type(media_type: &str, file_name: Option<&str>) -> &'static str {
    let extension = file_name
        .and_then(|name| std::path::Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match (media_type, extension.as_deref()) {
        ("image", Some("jpg" | "jpeg")) => "image/jpeg",
        ("image", Some("png")) => "image/png",
        ("image", Some("webp")) => "image/webp",
        ("audio", Some("ogg")) => "audio/ogg",
        ("audio", Some("wav")) => "audio/wav",
        ("audio", _) => "audio/mpeg",
        ("video", Some("webm")) => "video/webm",
        ("video", _) => "video/mp4",
        ("file", Some("pdf")) => "application/pdf",
        ("file", Some("json")) => "application/json",
        _ => "application/octet-stream",
    }
}

pub(crate) fn block_on_tool_future<F, T>(future: F) -> Result<T, ToolError>
where
    F: std::future::Future<Output = Result<T, ToolError>>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create runtime: {e}")))?;
        rt.block_on(future)
    }
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    origins: RwLock<HashMap<String, String>>,
    disabled: RwLock<std::collections::HashSet<String>>,
    disabled_origins: RwLock<std::collections::HashSet<String>>,
}

fn actor_can_see_tool(actor: Option<&str>, tool_name: &str) -> bool {
    const LOCAL_OPERATOR_ONLY: [&str; 3] = ["audit", "prompt_inspect", "memory_graph"];
    if LOCAL_OPERATOR_ONLY.contains(&tool_name) {
        actor == Some("local:default")
    } else {
        true
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            origins: RwLock::new(HashMap::new()),
            disabled: RwLock::new(std::collections::HashSet::new()),
            disabled_origins: RwLock::new(std::collections::HashSet::new()),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.register_arc(Arc::from(tool), None);
    }

    pub fn register_from_plugin(&mut self, plugin: &str, tool: Box<dyn Tool>) {
        self.register_arc(Arc::from(tool), Some(plugin.to_string()));
    }

    pub fn register_from_plugin_live(&self, plugin: &str, tool: Box<dyn Tool>) {
        self.register_arc_live(Arc::from(tool), Some(plugin.to_string()));
    }

    pub fn register_live(&self, tool: Box<dyn Tool>) {
        self.register_arc_live(Arc::from(tool), None);
    }

    fn register_arc(&mut self, tool: Arc<dyn Tool>, origin: Option<String>) {
        let name = tool.name().to_string();
        self.tools
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.clone(), tool);
        let origins = self
            .origins
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(origin) = origin {
            origins.insert(name, origin);
        } else {
            origins.remove(&name);
        }
    }

    fn register_arc_live(&self, tool: Arc<dyn Tool>, origin: Option<String>) {
        let name = tool.name().to_string();
        self.tools
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.clone(), tool);
        let mut origins = self
            .origins
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(origin) = origin {
            origins.insert(name, origin);
        } else {
            origins.remove(&name);
        }
    }

    pub fn unregister_plugin_tools(&self, plugin: &str) -> Vec<String> {
        let names: Vec<String> = {
            let origins = self
                .origins
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            origins
                .iter()
                .filter(|&(_name, origin)| origin == plugin)
                .map(|(name, _origin)| name.clone())
                .collect()
        };
        if names.is_empty() {
            return names;
        }
        {
            let mut tools = self
                .tools
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for name in &names {
                tools.remove(name);
            }
        }
        {
            let mut origins = self
                .origins
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for name in &names {
                origins.remove(name);
            }
        }
        names
    }

    pub fn unregister_prefixed_tools(&self, prefix: &str) -> Vec<String> {
        let names: Vec<String> = {
            let tools = self
                .tools
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tools
                .keys()
                .filter(|name| name.starts_with(prefix))
                .cloned()
                .collect()
        };
        if names.is_empty() {
            return names;
        }
        {
            let mut tools = self
                .tools
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for name in &names {
                tools.remove(name);
            }
        }
        {
            let mut origins = self
                .origins
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for name in &names {
                origins.remove(name);
            }
        }
        names
    }

    /// Move all tools from this registry into another.
    /// Tools already present in `target` are not overwritten.
    pub fn drain_into(&mut self, target: &mut Self) {
        let origins = self
            .origins
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tools = self
            .tools
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let drained: Vec<(String, Arc<dyn Tool>, Option<String>)> = tools
            .drain()
            .map(|(name, tool)| {
                let origin = origins.remove(&name);
                (name, tool, origin)
            })
            .collect();
        for (name, tool, origin) in drained {
            if target.get(&name).is_none() {
                target.register_arc(tool, origin);
            }
        }
    }

    /// Update the disabled set.  Replaces any previous filter.
    /// Disabled tools remain registered but are hidden from `get()`,
    /// `definitions()`, and `tool_names()`.  Safe to call from hot-reload.
    pub fn apply_disabled_filter(&self, disabled: &[String]) {
        if let Ok(mut guard) = self.disabled.write() {
            *guard = disabled.iter().cloned().collect();
        }
    }

    pub fn apply_plugin_enabled_filter(&self, enabled_plugins: &[String]) {
        let enabled: std::collections::HashSet<&str> =
            enabled_plugins.iter().map(String::as_str).collect();
        let disabled_origins: std::collections::HashSet<String> = self
            .origins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|origin| !enabled.contains(origin.as_str()))
            .cloned()
            .collect();
        if let Ok(mut guard) = self.disabled_origins.write() {
            *guard = disabled_origins;
        }
    }

    fn is_disabled(&self, name: &str) -> bool {
        if self.disabled.read().is_ok_and(|s| s.contains(name)) {
            return true;
        }
        let disabled_origins = {
            let guard = self
                .disabled_origins
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.is_empty() {
                return false;
            }
            guard.clone()
        };
        if disabled_origins.is_empty() {
            return false;
        }
        self.origins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .is_some_and(|origin| disabled_origins.contains(origin))
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if self.is_disabled(name) {
            return None;
        }
        self.tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    #[must_use]
    pub fn get_for_actor(&self, actor: Option<&str>, name: &str) -> Option<Arc<dyn Tool>> {
        if !actor_can_see_tool(actor, name) {
            return None;
        }
        self.get(name)
    }

    #[must_use]
    pub fn capabilities(&self, name: &str) -> Option<cortex_sdk::ToolCapabilities> {
        self.get(name).map(|tool| tool.capabilities())
    }

    /// Tool definitions for LLM, sorted by name (excludes disabled).
    #[must_use]
    pub fn definitions(&self) -> Vec<serde_json::Value> {
        self.definitions_for_actor(None)
    }

    /// Tool definitions for LLM filtered by the current actor context.
    #[must_use]
    pub fn definitions_for_actor(&self, actor: Option<&str>) -> Vec<serde_json::Value> {
        let mut definitions: Vec<(String, serde_json::Value)> = {
            let tools = self
                .tools
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tools
                .iter()
                .filter(|(name, _tool)| !self.is_disabled(name) && actor_can_see_tool(actor, name))
                .map(|(name, tool)| {
                    (
                        name.clone(),
                        serde_json::json!({
                            "name": tool.name(),
                            "description": tool.description(),
                            "input_schema": tool.input_schema(),
                        }),
                    )
                })
                .collect()
        };
        definitions.sort_by(|(left, _), (right, _)| left.cmp(right));
        definitions.into_iter().map(|(_name, def)| def).collect()
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_names_for_actor(None)
    }

    #[must_use]
    pub fn tool_names_for_actor(&self, actor: Option<&str>) -> Vec<String> {
        let mut names: Vec<String> = {
            let tools = self
                .tools
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tools
                .keys()
                .filter(|name| !self.is_disabled(name) && actor_can_see_tool(actor, name))
                .cloned()
                .collect()
        };
        names.sort();
        names
    }

    /// Total count of enabled tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .filter(|n| !self.is_disabled(n))
            .count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }
}

/// Register the core tool set for a cognitive runtime.
///
/// Includes file I/O (`read`, `write`, `edit`), execution (`bash`),
/// memory (`memory_search`, `memory_save`), delegation (`agent`),
/// and scheduling (`cron`).  The `skill` tool is registered separately
/// because it needs a `SkillRegistry`.  Plugin tools are loaded
/// separately via the plugin system.
///
/// `media_api_key` is the effective API key for media providers (resolved
/// from `media.api_key` or `api.api_key`).
pub fn register_core_tools(
    registry: &mut ToolRegistry,
    recall_ctx: std::sync::Arc<memory_tools::MemoryRecallComponents>,
    web_config: cortex_types::config::WebConfig,
    media_config: cortex_types::config::MediaConfig,
    media_api_key: String,
    cron_queue: std::sync::Arc<cron::CronQueue>,
) {
    // File I/O
    registry.register(Box::new(read::ReadTool));
    registry.register(Box::new(write::WriteTool));
    registry.register(Box::new(edit::EditTool));
    // Execution
    registry.register(Box::new(bash::BashTool));
    // Memory
    let store = recall_ctx.store.clone();
    registry.register(Box::new(memory_tools::MemorySearchTool::new(recall_ctx)));
    registry.register(Box::new(memory_tools::MemorySaveTool::new(store)));
    // Agent
    registry.register(Box::new(agent::AgentTool));
    // Scheduling
    registry.register(Box::new(cron::CronTool::new(cron_queue)));
    registry.register(Box::new(send_media::SendMediaTool));
    // Web
    let fetch_config = web_config.clone();
    registry.register(Box::new(web_search::WebSearchTool::new(web_config)));
    registry.register(Box::new(web_fetch::WebFetchTool::new(fetch_config)));
    // Media
    registry.register(Box::new(tts::TtsTool::new(
        media_config.clone(),
        media_api_key.clone(),
    )));
    registry.register(Box::new(image_gen::ImageGenTool::new(
        media_config.clone(),
        media_api_key.clone(),
    )));
    registry.register(Box::new(video_gen::VideoGenTool::new(
        media_config,
        media_api_key,
    )));
}

/// Register core tools for sub-agent contexts (no external dependencies).
///
/// Excludes tools that require runtime infrastructure unavailable to sub-agents:
/// - `memory_search`/`memory_save` — need embedding pipeline and memory store
/// - `cron` — needs persistent `CronQueue` (owned by parent daemon)
/// - `web_search` — needs `WebConfig` with API credentials
/// - `web_fetch` — needs async HTTP runtime (conflicts with scoped thread execution)
/// - `skill` — registered separately via `SkillRegistry`
pub fn register_core_tools_basic(registry: &mut ToolRegistry) {
    registry.register(Box::new(read::ReadTool));
    registry.register(Box::new(write::WriteTool));
    registry.register(Box::new(edit::EditTool));
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(agent::AgentTool));
    registry.register(Box::new(send_media::SendMediaTool));
}

/// Register memory tools (search + save) with full 6-dimensional hybrid recall capability.
pub fn register_memory_tools(
    registry: &mut ToolRegistry,
    ctx: std::sync::Arc<memory_tools::MemoryRecallComponents>,
) {
    let store = ctx.store.clone();
    registry.register(Box::new(memory_tools::MemorySearchTool::new(ctx)));
    registry.register(Box::new(memory_tools::MemorySaveTool::new(store)));
}

#[cfg(test)]
mod tests {
    use super::{Tool, ToolRegistry, ToolResult};

    struct NamedTool(&'static str);

    impl Tool for NamedTool {
        fn name(&self) -> &'static str {
            self.0
        }

        fn description(&self) -> &'static str {
            "test tool"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }

        fn execute(&self, _input: serde_json::Value) -> Result<ToolResult, super::ToolError> {
            Ok(ToolResult::success("ok".to_string()))
        }
    }

    #[test]
    fn local_operator_only_introspection_tools_are_hidden_from_non_local_actors() {
        let mut registry = ToolRegistry::new();
        for name in ["audit", "prompt_inspect", "memory_graph", "read"] {
            registry.register(Box::new(NamedTool(name)));
        }

        let non_local_names: Vec<String> = registry
            .definitions_for_actor(Some("user:scott"))
            .into_iter()
            .filter_map(|def| {
                def.get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        assert!(
            !non_local_names.iter().any(|name| {
                matches!(name.as_str(), "audit" | "prompt_inspect" | "memory_graph")
            }),
            "non-local actors should not see self-introspection tools: {non_local_names:?}"
        );
        assert!(
            non_local_names.iter().any(|name| name == "read"),
            "non-local actors should keep ordinary tools visible"
        );

        let local_names: Vec<String> = registry
            .definitions_for_actor(Some("local:default"))
            .into_iter()
            .filter_map(|def| {
                def.get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        for expected in ["audit", "prompt_inspect", "memory_graph", "read"] {
            assert!(
                local_names.iter().any(|name| name == expected),
                "local operator should keep {expected} visible"
            );
        }
    }
}
