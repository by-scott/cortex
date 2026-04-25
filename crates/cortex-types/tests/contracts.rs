use cortex_types::{
    AssistantResponse, ContentBlock, Event, MemoryEntry, MemoryKind, MemoryStatus, MemoryType,
    Message, NativePluginIsolation, Payload, PluginManifest, Role, TextFormat, TurnState,
    check_compatibility,
};

#[test]
fn turn_state_contract_allows_only_runtime_transitions() {
    assert!(
        TurnState::Idle
            .try_transition(TurnState::Processing)
            .is_ok()
    );
    assert!(
        TurnState::Processing
            .try_transition(TurnState::Completed)
            .is_ok()
    );
    assert!(
        TurnState::Completed
            .try_transition(TurnState::Processing)
            .is_err()
    );
    assert!(TurnState::Completed.is_terminal());
}

#[test]
fn message_and_response_contract_round_trips() {
    let message = Message::user("hello");
    assert_eq!(message.role, Role::User);
    assert!(matches!(
        message.content.first(),
        Some(ContentBlock::Text { text }) if text == "hello"
    ));

    let response = AssistantResponse {
        text: "done".to_string(),
        format: TextFormat::Markdown,
        parts: Vec::new(),
    };
    let encoded = match serde_json::to_string(&response) {
        Ok(value) => value,
        Err(err) => panic!("response should serialize: {err}"),
    };
    let decoded: AssistantResponse = match serde_json::from_str(&encoded) {
        Ok(value) => value,
        Err(err) => panic!("response should decode: {err}"),
    };
    assert_eq!(decoded.plain_text(), "done");
}

#[test]
fn memory_and_payload_contracts_keep_owner_and_shape() {
    let mut entry = MemoryEntry::new(
        "prefers concise updates",
        "user preference",
        MemoryType::User,
        MemoryKind::Semantic,
    );
    assert_eq!(entry.owner_actor, "local:default");
    entry.owner_actor = "telegram:42".to_string();
    assert_eq!(entry.status, MemoryStatus::Captured);
    assert_eq!(entry.owner_actor, "telegram:42");

    let payload = Payload::MemoryCaptured {
        memory_id: entry.id,
        memory_type: "user".to_string(),
    };
    let encoded = match rmp_serde::to_vec_named(&payload) {
        Ok(value) => value,
        Err(err) => panic!("payload should encode: {err}"),
    };
    let decoded: Payload = match rmp_serde::from_slice(&encoded) {
        Ok(value) => value,
        Err(err) => panic!("payload should decode: {err}"),
    };
    assert!(matches!(decoded, Payload::MemoryCaptured { .. }));

    let event = Event::new(
        cortex_types::TurnId::new(),
        cortex_types::CorrelationId::new(),
        payload,
    );
    assert_eq!(event.execution_version, cortex_types::EXECUTION_VERSION);
}

#[test]
fn plugin_manifest_requires_latest_version_field_and_process_default() {
    let manifest: PluginManifest = match toml::from_str(
        r#"
name = "sample"
version = "0.1.0"
description = "sample"
cortex_version = "1.3.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"
"#,
    ) {
        Ok(value) => value,
        Err(err) => panic!("manifest should parse: {err}"),
    };
    assert_eq!(
        manifest
            .native
            .as_ref()
            .map_or(NativePluginIsolation::TrustedInProcess, |native| {
                native.isolation
            }),
        NativePluginIsolation::Process
    );
    assert!(check_compatibility(&manifest, "1.3.0").compatible);

    let rejected = toml::from_str::<PluginManifest>(
        r#"
name = "sample"
version = "0.1.0"
description = "sample"
cortex_version_requirement = ">=1.3.0"
"#,
    );
    assert!(rejected.is_err());
}

#[test]
fn readme_event_variant_count_matches_payload_surface() {
    let event_source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("event.rs");
    let event_source = match std::fs::read_to_string(&event_source_path) {
        Ok(value) => value,
        Err(err) => panic!("event source should load: {err}"),
    };
    let payload_count = count_payload_variants(&event_source);

    let readme_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.md");
    let readme = match std::fs::read_to_string(&readme_path) {
        Ok(value) => value,
        Err(err) => panic!("README should load: {err}"),
    };
    let Some(reported_count) = extract_readme_event_variant_count(&readme) else {
        panic!("README should mention the event variant count");
    };

    assert_eq!(
        reported_count, payload_count,
        "README event variant count drifted from Payload surface"
    );

    let readme_zh_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.zh.md");
    let readme_zh = match std::fs::read_to_string(&readme_zh_path) {
        Ok(value) => value,
        Err(err) => panic!("README.zh should load: {err}"),
    };
    let Some(reported_count_zh) = extract_readme_event_variant_count(&readme_zh) else {
        panic!("README.zh should mention the event variant count");
    };

    assert_eq!(
        reported_count_zh, payload_count,
        "README.zh event variant count drifted from Payload surface"
    );
}

#[test]
fn readme_turn_state_count_matches_runtime_surface() {
    let turn_source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("turn.rs");
    let turn_source = match std::fs::read_to_string(&turn_source_path) {
        Ok(value) => value,
        Err(err) => panic!("turn source should load: {err}"),
    };
    let turn_state_count = count_enum_variants(&turn_source, "TurnState");

    let readme_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.md");
    let readme = match std::fs::read_to_string(&readme_path) {
        Ok(value) => value,
        Err(err) => panic!("README should load: {err}"),
    };

    assert_eq!(turn_state_count, 10, "TurnState contract drifted");
    assert!(
        readme.contains("A ten-state turn machine"),
        "README should mention the ten-state turn machine"
    );

    let readme_zh_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.zh.md");
    let readme_zh = match std::fs::read_to_string(&readme_zh_path) {
        Ok(value) => value,
        Err(err) => panic!("README.zh should load: {err}"),
    };
    assert!(
        readme_zh.contains("10 态 Turn 状态机"),
        "README.zh should mention the ten-state turn machine"
    );
}

#[test]
fn readme_attention_and_metacognition_surfaces_match_runtime() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let readme = read_doc(&repo_root.join("README.md"));
    let readme_zh = read_doc(&repo_root.join("README.zh.md"));
    let attention_source = read_doc(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("attention.rs"),
    );
    let monitor_source = read_doc(
        &repo_root
            .join("crates")
            .join("cortex-turn")
            .join("src")
            .join("meta")
            .join("monitor.rs"),
    );

    let attention_count = count_enum_variants(&attention_source, "AttentionChannel");
    let alert_count = count_enum_variants(&monitor_source, "AlertKind");

    assert_eq!(attention_count, 3, "AttentionChannel surface drifted");
    assert_eq!(alert_count, 5, "AlertKind surface drifted");
    assert!(
        readme.contains("Three attention channels (Foreground, Maintenance, Emergency)"),
        "README should list the current attention channels"
    );
    assert!(
        readme.contains(
            "Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded)"
        ),
        "README should list the current metacognitive detectors"
    );
    assert!(
        readme_zh.contains(
            "五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）"
        ),
        "README.zh should list the current metacognitive detectors"
    );
    assert!(
        readme_zh.contains("三个注意力通道（Foreground、Maintenance、Emergency）"),
        "README.zh should list the current attention channels"
    );
}

#[test]
fn readme_memory_recall_dimensions_match_runtime() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let readme = read_doc(&repo_root.join("README.md"));
    let readme_zh = read_doc(&repo_root.join("README.zh.md"));
    let recall_source = read_doc(
        &repo_root
            .join("crates")
            .join("cortex-turn")
            .join("src")
            .join("memory")
            .join("recall.rs"),
    );
    let recall_weight_count = count_const_prefix(&recall_source, "W_");

    assert_eq!(
        recall_weight_count, 6,
        "memory recall weight surface drifted"
    );
    assert!(
        readme.contains(
            "six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity)"
        ),
        "README should list the current memory recall dimensions"
    );
    assert!(
        readme_zh
            .contains("六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）"),
        "README.zh should list the current memory recall dimensions"
    );
}

#[test]
fn plugin_boundary_docs_match_manifest_surface() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let plugins_doc = read_doc(&repo_root.join("docs").join("plugins.md"));
    let plugins_doc_zh = read_doc(&repo_root.join("docs").join("zh").join("plugins.md"));
    let plugin_source = read_doc(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("plugin.rs"),
    );

    assert_plugin_docs_en(&plugins_doc);
    assert_plugin_docs_zh(&plugins_doc_zh);

    let process = match serde_json::to_string(&NativePluginIsolation::Process) {
        Ok(value) => value,
        Err(err) => panic!("process isolation should serialize: {err}"),
    };
    assert_eq!(process, "\"process\"");
    let trusted = match serde_json::to_string(&NativePluginIsolation::TrustedInProcess) {
        Ok(value) => value,
        Err(err) => panic!("trusted isolation should serialize: {err}"),
    };
    assert_eq!(trusted, "\"trusted_in_process\"");

    for field in [
        "working_dir",
        "allow_host_paths",
        "inherit_env",
        "timeout_secs",
        "max_output_bytes",
    ] {
        assert!(
            plugin_source.contains(field),
            "plugin manifest surface should still contain {field}"
        );
    }
    for phrase in [
        "working_dir",
        "inherit_env",
        "timeout_secs",
        "max_output_bytes",
    ] {
        assert!(
            plugins_doc.contains(phrase),
            "plugins.md should document {phrase}"
        );
        assert!(
            plugins_doc_zh.contains(phrase),
            "Chinese plugin docs should document {phrase}"
        );
    }
}

fn assert_plugin_docs_en(plugins_doc: &str) {
    assert!(
        plugins_doc.contains("process-isolated JSON tools"),
        "plugins.md should describe the process JSON boundary"
    );
    assert!(
        plugins_doc.contains("trusted native ABI"),
        "plugins.md should describe the trusted native ABI boundary"
    );
    assert!(
        plugins_doc.contains("cortex_plugin_init"),
        "plugins.md should name the stable native entrypoint"
    );
    assert!(
        plugins_doc.contains("allow_host_paths = true"),
        "plugins.md should document explicit host-path opt-in"
    );
    assert!(
        plugins_doc.contains("abi_version = 1"),
        "plugins.md should document the current native ABI version"
    );
    assert!(
        plugins_doc.contains("Cortex does not load Rust trait-object symbols"),
        "plugins.md should keep the native boundary wording in sync"
    );
    assert!(
        plugins_doc.contains("surfaces stderr as the tool error"),
        "plugins.md should describe non-zero process stderr propagation"
    );
    assert!(
        plugins_doc.contains("stdout is not valid JSON"),
        "plugins.md should describe invalid JSON output rejection"
    );
}

fn assert_plugin_docs_zh(plugins_doc_zh: &str) {
    assert!(
        plugins_doc_zh.contains("进程隔离 JSON 工具"),
        "Chinese plugin docs should describe the process JSON boundary"
    );
    assert!(
        plugins_doc_zh.contains("强信任 native ABI"),
        "Chinese plugin docs should describe the trusted native ABI boundary"
    );
    assert!(
        plugins_doc_zh.contains("cortex_plugin_init"),
        "Chinese plugin docs should name the stable native entrypoint"
    );
    assert!(
        plugins_doc_zh.contains("allow_host_paths = true"),
        "Chinese plugin docs should document explicit host-path opt-in"
    );
    assert!(
        plugins_doc_zh.contains("abi_version = 1"),
        "Chinese plugin docs should document the current native ABI version"
    );
    assert!(
        plugins_doc_zh.contains("stderr 作为工具错误返回"),
        "Chinese plugin docs should describe non-zero process stderr propagation"
    );
    assert!(
        plugins_doc_zh.contains("stdout 不是合法 JSON"),
        "Chinese plugin docs should describe invalid JSON output rejection"
    );
}

#[test]
fn replay_and_compaction_docs_match_event_surface() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let readme = read_doc(&repo_root.join("README.md"));
    let readme_zh = read_doc(&repo_root.join("README.zh.md"));
    let executive = read_doc(&repo_root.join("docs").join("executive.md"));
    let executive_zh = read_doc(&repo_root.join("docs").join("zh").join("executive.md"));
    let usage = read_doc(&repo_root.join("docs").join("usage.md"));
    let usage_zh = read_doc(&repo_root.join("docs").join("zh").join("usage.md"));
    let maturity = read_doc(&repo_root.join("docs").join("maturity.md"));
    let maturity_zh = read_doc(&repo_root.join("docs").join("zh").join("maturity.md"));
    let event_source = read_doc(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("event.rs"),
    );

    for marker in [
        "ContextCompactBoundary",
        "ExternalInputObserved",
        "GuardrailTriggered",
        "SideEffectRecorded",
        "ExternalizedPayload",
        "ProjectionCheckpoint",
        "SnapshotCreated",
    ] {
        assert!(
            event_source.contains(marker),
            "event surface should still contain {marker}"
        );
    }

    assert!(
        readme.contains("compaction boundaries, side-effect substitution, and replay digests"),
        "README should describe the current replay surface"
    );
    assert!(
        usage.contains("explicit compact boundary"),
        "usage docs should describe context compaction boundaries"
    );
    assert!(
        executive.contains("records the replacement history in the journal"),
        "executive docs should describe compact-boundary journal replacement history"
    );
    assert!(
        executive.contains("replay and continuity remain journaled"),
        "executive docs should keep replay continuity journal wording"
    );
    assert!(
        readme_zh.contains("压缩边界和重放输入都会进入 Journal"),
        "README.zh should describe the journaled replay boundary"
    );
    assert!(
        readme_zh.contains("确定性重放会在投影时替换已记录或 provider 提供的副作用值"),
        "README.zh should describe replay side-effect substitution"
    );
    assert!(
        usage_zh.contains("显式 compact boundary"),
        "Chinese usage docs should describe context compaction boundaries"
    );
    assert!(
        executive_zh.contains("并将替换后的历史写入 Journal"),
        "Chinese executive docs should describe compact-boundary journal replacement history"
    );
    assert!(
        executive_zh.contains("重放和连续性保持 journaled"),
        "Chinese executive docs should keep replay continuity journal wording"
    );
    assert!(
        maturity.contains("SideEffectRecorded"),
        "maturity docs should describe recorded side effects"
    );
    assert!(
        maturity.contains("suspicious tool outputs are journaled for audit"),
        "maturity docs should describe operator-visible suspicious tool output handling"
    );
    assert!(
        maturity_zh.contains("`SideEffectRecorded`"),
        "Chinese maturity docs should describe recorded side effects"
    );
    assert!(
        maturity_zh.contains("可疑工具输出会写入 Journal 供审计"),
        "Chinese maturity docs should describe operator-visible suspicious tool output handling"
    );
}

#[test]
fn risk_surface_docs_match_runtime_contracts() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let readme = read_doc(&repo_root.join("README.md"));
    let config = read_doc(&repo_root.join("docs").join("config.md"));
    let config_zh = read_doc(&repo_root.join("docs").join("zh").join("config.md"));
    let maturity = read_doc(&repo_root.join("docs").join("maturity.md"));
    let maturity_zh = read_doc(&repo_root.join("docs").join("zh").join("maturity.md"));
    let permission_source = read_doc(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("permission.rs"),
    );

    let risk_level_count = count_enum_variants(&permission_source, "RiskLevel");
    assert_eq!(risk_level_count, 4, "RiskLevel surface drifted");

    for level in ["Allow", "Review", "RequireConfirmation", "Block"] {
        assert!(
            permission_source.contains(level),
            "permission surface should still contain {level}"
        );
    }

    assert!(
        readme.contains("Unknown plugin and MCP tools are risk-scored conservatively and require confirmation by default."),
        "README should keep the conservative unknown-tool risk wording"
    );
    assert!(
        config.contains("`risk.deny` always wins."),
        "config docs should describe deny precedence"
    );
    assert!(
        config.contains("`risk.allow` is non-empty, tools not matching it are blocked"),
        "config docs should describe allowlist blocking semantics"
    );
    assert!(
        config.contains("`Block` still denies without prompting."),
        "config docs should describe the block risk level"
    );
    assert!(
        config_zh.contains("`risk.deny` 始终优先。"),
        "Chinese config docs should describe deny precedence"
    );
    assert!(
        config_zh.contains("未匹配的工具会被阻断"),
        "Chinese config docs should describe allowlist blocking semantics"
    );
    assert!(
        config_zh.contains("`Block` 仍然直接拒绝且不弹确认。"),
        "Chinese config docs should describe the block risk level"
    );
    assert!(
        maturity.contains("Unknown tools, including plugin and MCP tools without a specific profile, are treated conservatively and require confirmation by default."),
        "maturity docs should describe the unknown-tool risk baseline"
    );
    assert!(
        maturity_zh.contains("未知工具，包括没有专门 profile 的插件和 MCP 工具，现在默认按保守风险评分处理，并需要确认。"),
        "Chinese maturity docs should describe the unknown-tool baseline"
    );
    assert!(
        maturity.contains("Embedding vectors inherit ownership through memory ids rather than carrying separate actor metadata."),
        "maturity docs should keep the embedding-ownership caveat"
    );
    assert!(
        maturity_zh
            .contains("Embedding 向量通过 memory id 继承归属，而不是单独携带 actor 元数据。"),
        "Chinese maturity docs should keep the embedding-ownership caveat"
    );
}

#[test]
fn plugin_hot_reload_docs_match_runtime_boundary() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let plugins = read_doc(&repo_root.join("docs").join("plugins.md"));
    let plugins_zh = read_doc(&repo_root.join("docs").join("zh").join("plugins.md"));
    let readme = read_doc(&repo_root.join("README.md"));

    assert!(
        plugins.contains(
            "Process-isolated command implementation changes apply on the next tool invocation."
        ),
        "plugins docs should describe process-plugin hot application"
    );
    assert!(
        plugins.contains(
            "Installing or replacing a trusted native shared library requires a daemon restart"
        ),
        "plugins docs should describe the trusted native restart boundary"
    );
    assert!(
        plugins_zh.contains("进程隔离命令实现更新会在下一次工具调用生效。"),
        "Chinese plugin docs should describe process-plugin hot reload"
    );
    assert!(
        plugins_zh.contains("安装或替换强信任 native 共享库时，需要重启 daemon"),
        "Chinese plugin docs should describe the trusted native restart boundary"
    );
    assert!(
        readme.contains("Shared-library code changes still require a daemon restart."),
        "README should keep the trusted native restart wording"
    );
}

#[test]
fn compatibility_policy_docs_match_current_extension_surfaces() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let compatibility = read_doc(&repo_root.join("docs").join("compatibility.md"));
    let compatibility_zh = read_doc(&repo_root.join("docs").join("zh").join("compatibility.md"));
    let readme = read_doc(&repo_root.join("README.md"));
    let readme_zh = read_doc(&repo_root.join("README.zh.md"));
    let maturity = read_doc(&repo_root.join("docs").join("maturity.md"));
    let maturity_zh = read_doc(&repo_root.join("docs").join("zh").join("maturity.md"));

    for phrase in [
        "cortex_plugin_init",
        "abi_version",
        "strict`, `balanced`, `open",
        "process-plugin manifest surface",
        "replay semantics",
        "additive, breaking, or rejection-only",
        "restart, reinstall, or plugin rebuild",
    ] {
        assert!(
            compatibility.contains(phrase),
            "compatibility docs should mention {phrase}"
        );
    }

    for phrase in [
        "cortex_plugin_init",
        "abi_version",
        "`strict`、`balanced`、`open`",
        "process-plugin manifest",
        "replay 语义",
        "additive、breaking，还是 rejection-only",
        "restart、reinstall 或 plugin rebuild",
    ] {
        assert!(
            compatibility_zh.contains(phrase),
            "Chinese compatibility docs should mention {phrase}"
        );
    }

    assert!(
        readme.contains("[Compatibility Policy](docs/compatibility.md)"),
        "README should link the compatibility policy"
    );
    assert!(
        readme_zh.contains("[兼容性策略](docs/zh/compatibility.md)"),
        "README.zh should link the compatibility policy"
    );
    assert!(
        maturity.contains("[Compatibility Policy](compatibility.md)"),
        "maturity docs should link the compatibility policy"
    );
    assert!(
        maturity_zh.contains("[兼容性策略](compatibility.md)"),
        "Chinese maturity docs should link the compatibility policy"
    );
}

#[test]
fn roadmap_docs_describe_a_single_1_3_release_line() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let roadmap = read_doc(&repo_root.join("docs").join("roadmap.md"));
    let roadmap_zh = read_doc(&repo_root.join("docs").join("zh").join("roadmap.md"));

    assert!(
        roadmap.contains("The next shipped version should be `1.3.0`."),
        "roadmap should define a single next shipped version"
    );
    assert!(
        roadmap
            .contains("These are workstreams inside `1.3.0`, not separate future version numbers."),
        "roadmap should keep workstreams scoped to 1.3.0"
    );
    assert!(
        roadmap.contains("embedding visibility checks that recover ownership through memory ids"),
        "roadmap should mention embedding visibility ownership checks"
    );
    assert!(
        roadmap.contains("actor-scoped memory tool tests for `memory_search` and `memory_save`"),
        "roadmap should mention the memory tool ownership surface"
    );
    assert!(
        !roadmap.contains("## 1.4"),
        "roadmap should not present 1.4 as a concurrent release line"
    );
    assert!(
        !roadmap.contains("## 1.5"),
        "roadmap should not present 1.5 as a concurrent release line"
    );

    assert!(
        roadmap_zh.contains("下一个正式版本应该是 `1.3.0`。"),
        "Chinese roadmap should define a single next shipped version"
    );
    assert!(
        roadmap_zh.contains("它们是 `1.3.0` 内部的工作流，而不是三个不同的未来版本号。"),
        "Chinese roadmap should keep workstreams scoped to 1.3.0"
    );
    assert!(
        roadmap_zh.contains("通过 memory id 恢复 embedding visibility 的校验"),
        "Chinese roadmap should mention embedding visibility ownership checks"
    );
    assert!(
        roadmap_zh
            .contains("面向 `memory_search` / `memory_save` 的 actor-scoped memory tool tests"),
        "Chinese roadmap should mention the memory tool ownership surface"
    );
    assert!(
        !roadmap_zh.contains("## 1.4"),
        "Chinese roadmap should not present 1.4 as a concurrent release line"
    );
    assert!(
        !roadmap_zh.contains("## 1.5"),
        "Chinese roadmap should not present 1.5 as a concurrent release line"
    );
}

fn extract_readme_event_variant_count(readme: &str) -> Option<usize> {
    for marker in ["event variants", "种事件变体"] {
        if let Some(index) = readme.find(marker) {
            let prefix = &readme[..index];
            let digits_rev: String = prefix
                .chars()
                .rev()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect();
            if !digits_rev.is_empty() {
                return digits_rev.chars().rev().collect::<String>().parse().ok();
            }
        }
    }
    None
}

fn count_payload_variants(source: &str) -> usize {
    count_enum_variants(source, "Payload")
}

fn count_enum_variants(source: &str, enum_name: &str) -> usize {
    let mut in_payload = false;
    let mut variant_count = 0usize;
    let mut depth = 0usize;
    let enum_header = format!("pub enum {enum_name} {{");

    for line in source.lines() {
        let trimmed = line.trim();
        if !in_payload {
            if trimmed == enum_header {
                in_payload = true;
                depth = 1;
            }
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("//") {
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            continue;
        }

        if depth == 1
            && !trimmed.starts_with('}')
            && trimmed
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            variant_count += 1;
        }

        depth += line.matches('{').count();
        depth = depth.saturating_sub(line.matches('}').count());
        if depth == 0 {
            break;
        }
    }

    variant_count
}

fn count_const_prefix(source: &str, prefix: &str) -> usize {
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("const "))
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
        .filter(|name| name.starts_with(prefix))
        .count()
}

fn read_doc(path: &std::path::Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(err) => panic!("failed to read {}: {err}", path.display()),
    }
}
