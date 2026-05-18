use super::{DaemonState, RuntimeBindings};

impl crate::hot_reload::ReloadTarget for DaemonState {
    fn reload_config(&self) {
        let paths = self.paths();
        let files = paths.config_files();
        let Ok(content) = std::fs::read_to_string(&files.config) else {
            return;
        };
        if toml::from_str::<cortex_types::config::CortexConfig>(&content).is_err() {
            tracing::warn!("Config reload: parse error, keeping current config");
            return;
        }

        let (new_providers, resolved) = match cortex_kernel::load_providers_for_paths(&paths) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!("Providers reload failed, keeping current providers: {err}");
                return;
            }
        };
        let new_config =
            cortex_kernel::load_config_for_paths(&paths, resolved.as_deref(), &new_providers);
        let old_config = self
            .config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let RuntimeBindings {
            actor_aliases,
            transport_actors,
            ..
        } = Self::load_runtime_bindings(&self.data_dir);

        if old_config.api.provider != new_config.api.provider
            || old_config.api.model != new_config.api.model
            || old_config.api.api_key != new_config.api.api_key
        {
            tracing::warn!("Config: LLM provider/model/key changed — restart to apply");
        }

        self.tools.apply_disabled_filter(&new_config.tools.disabled);
        self.tools
            .apply_plugin_enabled_filter(&new_config.plugins.enabled);
        *self
            .actor_aliases
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = actor_aliases;
        *self
            .transport_actors
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = transport_actors;

        if let Ok(mut guard) = self.config.write() {
            *guard = new_config.clone();
        }

        if let Ok(mut guard) = self.providers.write() {
            *guard = new_providers;
        }

        if old_config.plugins.enabled != new_config.plugins.enabled {
            let warnings = crate::plugin_loader::reload_process_plugin_tools(
                self.home(),
                &new_config.plugins,
                &self.tools,
            );
            for warning in warnings {
                tracing::warn!(plugin_warning = %warning, "plugin hot-reload warning");
            }
            tracing::info!("Plugin enablement hot-reloaded");
        }

        self.tools.unregister_prefixed_tools("mcp_");
        if !new_config.mcp.servers.is_empty() {
            let warnings = tokio::runtime::Handle::try_current().map_or_else(
                |_| match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime.block_on(async {
                        let mcp_manager = cortex_turn::mcp::McpManager::new();
                        mcp_manager
                            .connect_and_register_live(&new_config.mcp, &self.tools)
                            .await
                    }),
                    Err(err) => {
                        tracing::warn!("MCP hot-reload runtime init failed: {err}");
                        Vec::new()
                    }
                },
                |handle| {
                    tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            let mcp_manager = cortex_turn::mcp::McpManager::new();
                            mcp_manager
                                .connect_and_register_live(&new_config.mcp, &self.tools)
                                .await
                        })
                    })
                },
            );
            for warning in warnings {
                tracing::warn!("MCP: {warning}");
            }
        }
        if toml::to_string(&old_config.mcp).ok() != toml::to_string(&new_config.mcp).ok() {
            tracing::info!("MCP tools hot-reloaded");
        }

        tracing::info!("Config reloaded");
    }

    fn restore_config(&self) {
        let paths = self.paths();
        let files = paths.config_files();
        if !files.config.exists() {
            let empty = cortex_types::config::ProviderRegistry::new();
            let _ = cortex_kernel::load_config_for_paths(&paths, None, &empty);
            tracing::warn!("config.toml deleted — restored default");
        }
        if !files.providers.exists() {
            let _ = cortex_kernel::load_providers_for_paths(&paths);
            tracing::warn!("providers.toml deleted — restored default");
        }
        self.reload_config();
    }

    fn reload_prompts(&self) {
        self.prompt_manager.reload();
    }

    fn on_prompt_deleted(&self, path: &std::path::Path) {
        tracing::warn!(
            "Prompt file deleted: {} (not auto-restored — edit is intentional)",
            path.display()
        );
        self.prompt_manager.reload();
    }

    fn reload_skills(&self) {
        self.skill_registry.reload_from(
            &self.paths().skills_dir(),
            &cortex_types::SkillSource::Instance,
        );
    }

    fn on_skill_deleted(&self, path: &std::path::Path) {
        tracing::warn!(
            "Skill file deleted: {} (not auto-restored — edit is intentional)",
            path.display()
        );
        self.reload_skills();
    }

    fn on_plugins_changed(&self, path: &std::path::Path) {
        let cfg = self.config().plugins.clone();
        let warnings =
            crate::plugin_loader::reload_process_plugin_tools(self.home(), &cfg, &self.tools);
        for warning in warnings {
            tracing::warn!(plugin_warning = %warning, "plugin hot-reload warning");
        }
        tracing::info!(
            path = %path.display(),
            "Plugin file changed; process-isolated tools reloaded where possible. In-process native libraries still require daemon restart."
        );
    }
}
