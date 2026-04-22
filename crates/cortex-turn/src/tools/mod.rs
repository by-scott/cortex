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
    tools: HashMap<String, Box<dyn Tool>>,
    disabled: std::sync::RwLock<std::collections::HashSet<String>>,
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
            tools: HashMap::new(),
            disabled: std::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Move all tools from this registry into another.
    /// Tools already present in `target` are not overwritten.
    pub fn drain_into(&mut self, target: &mut Self) {
        for (name, tool) in self.tools.drain() {
            if target.get(&name).is_none() {
                target.tools.insert(name, tool);
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

    fn is_disabled(&self, name: &str) -> bool {
        self.disabled.read().is_ok_and(|s| s.contains(name))
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        if self.is_disabled(name) {
            return None;
        }
        self.tools.get(name).map(AsRef::as_ref)
    }

    /// Tool definitions for LLM, sorted by name (excludes disabled).
    #[must_use]
    pub fn definitions(&self) -> Vec<serde_json::Value> {
        let mut names: Vec<&str> = self
            .tools
            .keys()
            .filter(|n| !self.is_disabled(n))
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
            .iter()
            .filter_map(|name| {
                let tool = self.tools.get(*name)?;
                Some(serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                }))
            })
            .collect()
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tools
            .keys()
            .filter(|n| !self.is_disabled(n))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// Total count of enabled tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.keys().filter(|n| !self.is_disabled(n)).count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
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
    use super::*;

    #[test]
    fn core_tools_basic_count() {
        let mut r = ToolRegistry::new();
        register_core_tools_basic(&mut r);
        // read, write, edit, bash, agent, send_media = 6
        assert_eq!(r.len(), 6, "got {}", r.len());
    }

    #[test]
    fn definitions_sorted() {
        let mut r = ToolRegistry::new();
        register_core_tools_basic(&mut r);
        let defs = r.definitions();
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|d| d.get("name").and_then(serde_json::Value::as_str))
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }
}
