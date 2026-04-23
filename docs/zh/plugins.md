# 插件开发指南

本指南涵盖从零开始构建、打包和分发原生 Cortex 插件的完整流程。

## 概述

Cortex 插件是使用 `cortex-sdk` crate 构建的原生共享库（Linux `.so`，macOS `.dylib`）。插件向 Cortex 实例贡献工具、Skills、Prompt 文件和结构化媒体附件，不依赖任何 Cortex 内部 crate。运行时在 Daemon 启动时通过 `dlopen` 加载插件，并将其工具注册到全局注册表。

### 插件可贡献什么

- **工具** — LLM 在 Turn 期间可调用的原生函数
- **Skills** — 按模式激活的 SKILL.md 认知策略
- **Prompts** — 与系统和实例 Prompt 一起加载的 Prompt 覆盖文件
- **媒体** — 由活跃客户端频道投递的结构化图片、音频、视频或文件附件

## 前置条件

- Rust（edition 2024）
- `cortex-sdk` crate（已发布在 crates.io）
- 运行中的 Cortex 实例（用于测试）

如果从零开始，先安装 Cortex。`cortex` 二进制既是运行时，也是插件工具链：创建脚手架、安装本地插件、打包 `.cpx`、从 GitHub Release 安装发布资产都由它完成。

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
cortex --version
```

## 项目搭建

### 使用脚手架

```bash
cortex scaffold my-tool
cd cortex-plugin-my-tool
```

生成 `cortex-plugin-my-tool/` 项目，包含 `Cargo.toml`、`manifest.toml`、`src/lib.rs`、`skills/`、`prompts/` 和起步 `README.md`。脚手架刻意保持最小形状：保留生成结构，再把示例工具替换为你的领域工具。

即使你熟悉 Rust，也建议从脚手架开始，因为它会统一 crate 名称、manifest 名称、原生库路径和打包约定。

### 手动搭建

#### Cargo.toml

```toml
[package]
name = "cortex-plugin-example"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
cortex-sdk = "1.0"
serde_json = "1"
```

`cdylib` crate 类型产生适合 FFI 加载的共享库。

不要依赖 Cortex 内部 crate。可分发插件应只依赖 `cortex-sdk` 和普通生态 crate。SDK 是兼容性边界。

#### 目录结构

```
cortex-plugin-example/
├── Cargo.toml
├── manifest.toml          # 插件元数据（必需）
├── src/
│   └── lib.rs             # 插件入口点
├── skills/                # 可选：Skill 定义
│   └── my-skill/
│       └── SKILL.md
└── prompts/               # 可选：Prompt 覆盖
```

#### manifest.toml

每个插件需要一个清单：

```toml
name = "example"
version = "0.1.0"
description = "这个插件做什么"
cortex_version = "1.1.0"

[capabilities]
provides = ["tools", "skills"]   # 可选 "tools"、"skills"、"prompts"

[native]
library = "lib/libcortex_plugin_example.so"  # 安装目录内的路径
entry = "cortex_plugin_create_multi"         # FFI 入口点名称（export_plugin! 生成）
```

## 编写插件

### 最小示例

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct MyPlugin;

impl MultiToolPlugin for MyPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "example".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "示例插件".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(WordCountTool)]
    }
}

struct WordCountTool;

impl Tool for WordCountTool {
    fn name(&self) -> &'static str { "word_count" }

    fn description(&self) -> &'static str {
        "统计文本中的单词数。"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "要统计的文本"
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("缺少 'text'".into()))?;
        Ok(ToolResult::success(format!("{} 个单词", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(MyPlugin);
```

### 工具设计指南

**`name`**：小写加下划线（`word_count`，不是 `WordCount`）。在所有工具中必须唯一。

**`description`**：为 LLM 编写。说明工具做什么、何时使用、何时 _不_ 使用。LLM 读取此描述来决定是否调用。

**`input_schema`**：描述参数的 JSON Schema 对象。LLM 生成匹配此 schema 的 JSON。

**`execute`**：接收 LLM 生成的 JSON。正常输出返回 `ToolResult::success`，可恢复错误返回 `ToolResult::error`。不可恢复的失败返回 `ToolError`。

### 结构化媒体

工具可以在不依赖 Cortex 内部 crate 的情况下附加媒体。返回给 LLM 的仍是文本；文件通过 SDK 自有的 `Attachment` DTO 附加：

```rust
fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| ToolError::InvalidInput("缺少 'path'".into()))?;

    Ok(ToolResult::success("已准备图片").with_media(Attachment {
        media_type: "image".into(),
        mime_type: "image/png".into(),
        url: path.into(),
        caption: None,
        size: std::fs::metadata(path).ok().map(|m| m.len()),
    }))
}
```

Cortex 在工具执行后收集这些附件，并通过 HTTP、WebSocket、Telegram、QQ 等频道共用的响应管线投递。工具不应直接调用频道专用 API。

### 运行时感知工具

需要会话上下文或要发出进度的工具可以重写 `execute_with_runtime`：

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    runtime: &dyn ToolRuntime,
) -> Result<ToolResult, ToolError> {
    let ctx = runtime.invocation();
    // ctx.session_id、ctx.actor、ctx.source、ctx.execution_scope

    runtime.emit_progress("第 1 步：处理中...");
    runtime.emit_observer(Some("my_tool"), "诊断信息");

    Ok(ToolResult::success("完成"))
}

fn capabilities(&self) -> ToolCapabilities {
    ToolCapabilities {
        emits_progress: true,
        emits_observer_text: true,
        background_safe: false,
    }
}
```

### SDK 接口

| 类型 | 用途 |
|------|------|
| `MultiToolPlugin` | FFI 入口点——返回插件信息和工具 |
| `Tool` | 单个工具接口（name、description、schema、execute）|
| `ToolResult` | 返回给 LLM 的成功或错误输出，以及可选结构化媒体 |
| `Attachment` | SDK 自有媒体 DTO：图片、音频、视频或文件 |
| `ToolError` | 不可恢复的失败（无效输入、执行失败）|
| `InvocationContext` | 稳定元数据：会话 ID、Actor、来源、执行作用域 |
| `ToolRuntime` | 运行时桥接：发出进度、发出观察者文本 |
| `ToolCapabilities` | 声明式标志：emits_progress、emits_observer_text、background_safe |
| `ExecutionScope` | Foreground（用户 Turn）或 Background（维护）|
| `PluginInfo` | 插件名称、版本、描述 |
| `export_plugin!` | 生成 FFI 入口点的宏 |

### 共享状态

工具是 `Send + Sync` 的——单个实例跨所有 Turn 共享。使用 `Arc<Mutex<T>>` 或 `Arc<RwLock<T>>` 管理可变状态。按 Actor 或会话（通过 `InvocationContext` 获取）命名空间化状态，防止跨用户泄漏。

## 编写 Skills

Skills 是带 YAML frontmatter 的 SKILL.md 文件：

```markdown
---
description: 这个 Skill 做什么
when_to_use: 何时激活
required_tools:
  - tool_name
tags:
  - category
activation:
  input_patterns:
    - (?i)(触发|关键词)
---

# Skill 名称

${ARGS}

## 第一阶段

这个阶段做什么...

## 第二阶段

接下来做什么...
```

### 激活机制

| 字段 | 类型 | 说明 |
|------|------|------|
| `input_patterns` | 正则数组 | 匹配用户输入 |
| `pressure_above` | 字符串 | 上下文压力超过指定级别时激活 |
| `alert_kinds` | 字符串数组 | 元认知警报触发时激活 |
| `event_kinds` | 字符串数组 | 特定事件类型触发时激活 |

## 构建

```bash
docker compose -f /path/to/cortex/docker-compose.yml run --rm \
  -v "$PWD:/plugin" -w /plugin dev cargo build --release
```

产出 `target/release/libcortex_plugin_example.so`（Linux）或 `.dylib`（macOS）。

## 打包 (.cpx)

构建后直接从项目根目录打包——打包器根据 manifest 中的 `[native].library` 字段自动从 `target/release/` 定位原生库：

```bash
docker compose -f /path/to/cortex/docker-compose.yml run --rm \
  -v "$PWD:/plugin" -w /plugin dev cargo build --release
cortex plugin pack .
```

无需 staging 目录。打包器读取 `manifest.toml`，在 `target/release/` 中找到 `.so`/`.dylib`，并包含 `skills/` 和 `prompts/`（如果存在）。
默认归档名为 `{仓库名}-v{manifest.version}-{platform}.cpx`，例如 `cortex-plugin-example-v0.1.0-linux-amd64.cpx`。

发布前始终先验证打包资产：

```bash
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx
cortex restart
cortex plugin list
```

## 安装

```bash
# 从 .cpx 文件
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx

# 从目录
cortex plugin install ./my-plugin/

# 从 GitHub
cortex plugin install your-name/cortex-plugin-example
```

## 通过 GitHub 分发

`cortex plugin install owner/repo` 会读取最新 GitHub Release，并下载其中的 `.cpx` 资产。也支持固定版本安装：

```bash
cortex plugin install your-name/cortex-plugin-example@1.1.0
cortex plugin install your-name/cortex-plugin-example@v1.1.0
```

分发步骤：
1. 构建 `.cpx` 归档
2. 在仓库上创建 GitHub Release
3. 将 `.cpx` 文件作为 Release 资产附加
4. 用户安装：`cortex plugin install your-name/cortex-plugin-example`

使用 GitHub CLI：

```bash
git tag v0.1.0
git push origin main --tags
gh release create v0.1.0 \
  ./cortex-plugin-example-v0.1.0-linux-amd64.cpx \
  --title "cortex-plugin-example v0.1.0" \
  --notes "Initial release."
```

### 命名约定

仓库名称应为 `cortex-plugin-{name}`。`manifest.toml` 中的 `name` 字段应为 `{name}`（不含 `cortex-plugin-` 前缀）。Release 资产应使用 `{仓库名}-v{版本}-{platform}.cpx`。

### 发布检查清单

- `Cargo.toml` package version 与 `manifest.toml` version 一致。
- `manifest.toml` `[native].library` 与实际构建出的库名一致。
- `cargo build --release` 在 Cortex 运行的目标环境中通过。
- `cortex plugin pack .` 生成 `{仓库名}-v{版本}-{platform}.cpx`。
- `cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx` 本地安装成功。
- `cortex plugin list` 显示预期插件名、版本和 native 标记。
- GitHub Release 包含版本化 `.cpx` 资产。
- 干净实例中 `cortex plugin install owner/repo@version` 可用。

## 插件生命周期

1. **加载** — Daemon 启动时 `dlopen`
2. **创建** — 运行时调用 `export_plugin!` 生成的 FFI 函数
3. **注册** — `create_tools()` 调用一次；每个工具进入全局注册表
4. **执行** — LLM 在 Turn 期间按名称调用工具；运行时以 JSON 调用 `execute`
5. **保持** — 库句柄在 Daemon 生命周期内保持；`Drop` 仅在关闭时运行

## 插件存储

插件安装到 `~/.cortex/plugins/{name}/`：

```
~/.cortex/plugins/example/
├── manifest.toml
├── lib/
│   └── libcortex_plugin_example.so
├── skills/
│   └── my-skill/
│       └── SKILL.md
└── prompts/
```

在 `config.toml` 中按实例启用。

## 管理命令

```bash
cortex plugin install owner/repo          # 从 GitHub 安装
cortex plugin install owner/repo@1.1.0    # 安装指定版本
cortex plugin install ./plugin.cpx        # 从 .cpx 安装
cortex plugin install ./plugin-dir        # 从目录安装
cortex plugin uninstall name              # 移除插件
cortex plugin list                        # 列出已安装插件
cortex plugin pack ./dir                  # 打包为 {仓库名}-v{版本}-{platform}.cpx
```

## 故障排查

| 现象 | 处理 |
|------|------|
| `manifest.toml missing 'name' field` | 确认归档根目录包含 `manifest.toml`，而不是套了一层项目目录 |
| 找不到原生库 | 检查 `[native].library`，先运行 `cargo build --release`，再从项目根目录打包 |
| 插件安装成功但工具不出现 | 重启 Cortex daemon；插件在 daemon 启动时加载 |
| GitHub 安装找不到资产 | 在 Release 中附加 `.cpx` 文件，推荐 `{仓库名}-v{版本}-{platform}.cpx` |
| 固定版本安装失败 | 版本可写 `1.1.0` 或 `v1.1.0`；Cortex 会将 `1.1.0` 规范化为 `v1.1.0` |

## 官方插件

官方开发插件提供 42 个工具和 13 个工作流 Skill：

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

参见 [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 作为参考实现。

## 发布 cortex-sdk

本节供 Cortex 维护者发布 SDK crate。普通插件作者不需要执行这些命令。

SDK crate 位于 `crates/cortex-sdk/`，必须保持与 Cortex 内部零耦合。公开 API 应是稳定 trait 或 DTO，第三方插件应能只依赖 SDK 编译，而不链接运行时内部模块。

发布前：

```bash
cargo fmt --all -- --check
cargo clippy -p cortex-sdk --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo test -p cortex-sdk
cargo publish -p cortex-sdk --dry-run
```

发布：

```bash
cargo publish -p cortex-sdk
```

发布后，从干净插件项目验证：

```bash
cargo new --lib cortex-plugin-smoke
cd cortex-plugin-smoke
```

设置 `crate-type = ["cdylib"]`，添加 `cortex-sdk = "1.0"` 和 `serde_json = "1"`，实现最小 `MultiToolPlugin`，然后运行：

```bash
cargo check
cargo build --release
cortex plugin pack .
```

发布规则：

- 保持 `crates/cortex-sdk/README.md` 与本文档同步。
- 保持 `Cargo.toml` 版本、发布说明和公开 API 变更一致。
- 不要通过 SDK 暴露 Cortex 内部模块。
- 不要在同一主版本下发布破坏性 API。
