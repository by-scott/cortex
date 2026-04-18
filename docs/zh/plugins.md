# 插件使用与开发指南

本指南涵盖如何安装和使用插件，以及如何开发和发布自己的插件。

---

## 概览

插件可通过最多 5 种能力类型扩展 Cortex：

| 能力 | 描述 |
|------|------|
| **工具（Tools）** | 在轮次中可供 LLM 使用的新工具 |
| **技能（Skills）** | 用于结构化思考的推理协议 |
| **提示（Prompts）** | 覆盖内置提示模板 |
| **LLM** | 新 LLM 服务的提供商后端 |
| **记忆（Memory）** | 记忆持久化的存储后端 |

核心运行时自带 17 个工具和 5 个技能。插件添加更多。默认不安装任何插件。

---

## 安装插件

### 安装来源

| 来源 | 命令 |
|------|------|
| GitHub | `cortex plugin install owner/repo` |
| URL | `cortex plugin install https://example.com/plugin.cpx` |
| 本地 .cpx | `cortex plugin install ./my-plugin.cpx` |
| 本地目录 | `cortex plugin install ./my-plugin/` |

### 按实例启用

安装后，在实例配置中将插件名称添加到启用列表：

```toml
# ~/.cortex/<instance>/config.toml
[plugins]
enabled = ["my-plugin"]
```

重启守护进程使更改生效。

### 管理插件

```bash
cortex plugin list                        # 列出已安装的插件
cortex plugin uninstall my-plugin         # 移除但保留文件
cortex plugin uninstall my-plugin --purge # 删除全部
```

### 官方插件：cortex-plugin-dev

[cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 插件是官方开发工具包，提供代码导航（tree-sitter）、git 操作、docker 管理、任务追踪、HTTP、SQL、LSP 和工作流技能等工具。安装方式：

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

---

## 插件存储

插件全局安装到 `~/.cortex/plugins/<name>/`，通过 `config.toml` 按实例启用。

```
~/.cortex/plugins/<name>/
  manifest.toml                   # 必需：元数据和能力声明
  skills/
    <skill-name>/SKILL.md         # 技能定义
  prompts/
    <template>.md                 # 提示模板覆盖
  lib/
    lib<name>.so                  # 原生共享库（可选）
```

---

## .cpx 归档格式

`.cpx` 文件是包含插件目录结构的 gzip 压缩 tar 归档。创建方式：

```bash
cortex plugin pack ./my-plugin/               # 输出：my-plugin.cpx
cortex plugin pack ./my-plugin/ custom.cpx    # 自定义输出名称
```

归档根目录必须包含 `manifest.toml`。其他目录（`lib/`、`skills/`、`prompts/`）可选。

通过 GitHub 分发时，创建 release 并附加 `.cpx` 作为 release asset。用户安装方式：

```bash
cortex plugin install owner/repo
```

---

## 技能加载：3 层层级

技能通过 3 层优先级系统加载。技能名称冲突时，高层覆盖低层：

```
system（随核心版本管理）< plugin（来自已启用插件）< instance（用户创建 + 演进）
```

| 层级 | 来源 | 位置 |
|------|------|------|
| System | 内置于核心 | 实例目录中的 `skills/system/` |
| Plugin | 从已启用插件加载 | `~/.cortex/plugins/<name>/skills/` |
| Instance | 用户创建或演进的 | 实例目录中的 `skills/`（非 system） |

插件技能标记为 `SkillSource::Plugin` 用于来源追踪。

---

## 开发插件

### 步骤 1：创建 manifest.toml

每个插件都需要一个声明元数据和能力的清单：

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "What this plugin does"
author = "Your Name"
cortex_version = "0.8"

[capabilities]
provides = ["tools", "skills"]
```

#### 清单字段

| 字段 | 必填 | 描述 |
|------|------|------|
| `name` | 是 | 唯一插件标识符（用于 `[plugins].enabled`） |
| `version` | 是 | 插件的语义版本 |
| `description` | 是 | 人类可读的摘要 |
| `author` | 是 | 作者名称或组织 |
| `cortex_version` | 是 | 所需的最低 Cortex 版本（major.minor） |
| `provides` | 是 | 能力类型数组：`tools`、`skills`、`prompts`、`llm`、`memory` |

### 步骤 2：添加技能（可选）

创建带有 YAML frontmatter 的 `skills/<skill-name>/SKILL.md`：

```markdown
---
description: Short description of what this skill does
when_to_use: When the LLM should activate this skill
required_tools:
  - bash
  - read
tags:
  - analysis
  - debugging
activation:
  input_patterns:
    - "debug.*crash"
    - "why is.*failing"
  alert_kinds:
    - error_spike
  event_kinds:
    - tool_failure
parameters:
  max_depth: 5
  verbose: false
execution_mode: interactive
---

# Skill Name

Skill execution instructions go here. This is the prompt template
that guides the LLM through the reasoning protocol.
```

#### 技能 Frontmatter 字段

| 字段 | 描述 |
|------|------|
| `description` | 用于列表和发现的简短摘要 |
| `when_to_use` | 激活的自然语言指导 |
| `required_tools` | 技能工作所需的工具 |
| `tags` | 分类标签 |
| `activation.input_patterns` | 触发技能建议的正则模式 |
| `activation.alert_kinds` | 触发建议的元认知警报类型 |
| `activation.event_kinds` | 触发建议的系统事件 |
| `parameters` | 传递给技能的默认参数值 |
| `execution_mode` | 技能运行方式（如 `interactive`、`autonomous`） |

### 步骤 3：添加提示覆盖（可选）

在 `prompts/` 中放置 Markdown 文件以覆盖 18 个内置系统模板中的任意一个。文件名必须与被覆盖的模板匹配：

`bootstrap.md`、`bootstrap-init.md`、`self-update.md`、`memory-extract.md`、`memory-consolidate.md`、`entity-extract.md`、`context-compress.md`、`context-summarize.md`、`causal-analyze.md`、`agent-readonly.md`、`agent-full.md`、`agent-teammate.md`、`hint-doom-loop.md`、`hint-fatigue.md`、`hint-frame-anchoring.md`、`hint-exploration.md`、`batch-analysis.md`、`summarize-system.md`

### 步骤 4：实现原生工具（可选）

对于提供工具的插件，实现 `MultiToolPlugin` FFI 接口。这是 `cortex-sdk` 中定义的核心接口，允许单个共享库通过一个入口点暴露多个工具。

#### cortex-sdk

`cortex-sdk` crate 是用于插件开发的官方 SDK。它是一个**零内部依赖的独立 crate** — 从零开始定义 `Tool`、`ToolResult`、`ToolError`、`MultiToolPlugin` 和 `PluginInfo` 等 trait/类型（约 150 行，纯接口）。它**独立发布到 crates.io**，插件作者无需访问闭源的内部 crate。内部的 `cortex-turn` crate 依赖 `cortex-sdk` 并重新导出其 trait；插件**仅依赖** `cortex-sdk`。

#### MultiToolPlugin Trait

```rust
use cortex_sdk::prelude::*;

pub trait MultiToolPlugin: Send + Sync {
    fn plugin_info(&self) -> PluginInfo;
    fn create_tools(&self) -> Vec<Box<dyn Tool>>;
}
```

每个工具实现标准 `Tool` trait：

```rust
use cortex_sdk::prelude::*;

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError>;
    fn timeout_secs(&self) -> Option<u64> { None }  // 可选超时覆盖
}
```

#### FFI 入口点

使用 `cortex-sdk` 的 `export_plugin!` 宏生成 FFI 入口点：

```rust
use cortex_sdk::prelude::*;

export_plugin!(MyPlugin);
```

宏展开为 `cortex_plugin_create_multi` C 函数。手动等效代码：

```rust
#[unsafe(no_mangle)]
pub extern "C" fn cortex_plugin_create_multi() -> *mut dyn MultiToolPlugin {
    Box::into_raw(Box::new(MyPlugin::default()))
}
```

运行时调用 `cortex_plugin_create_multi()`，获取 `MultiToolPlugin` trait 对象，然后调用 `create_tools()` 将每个工具注册到全局工具注册表中。

#### Cargo.toml 设置

你的插件 crate 必须是 `cdylib`：

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
cortex-sdk = "1.0"
serde_json = "1"
```

#### 构建库

```bash
cargo build --release
mkdir -p my-plugin/lib
cp target/release/libmy_plugin.so my-plugin/lib/
```

将编译好的 `.so` 放入插件的 `lib/` 目录。

### 步骤 5：插件库生命周期

理解生命周期对原生插件作者很重要：

1. **加载**：插件共享库在守护进程启动时通过 `dlopen` 加载
2. **创建**：运行时调用 `cortex_plugin_create_multi()` 获取 `MultiToolPlugin` 实例
3. **注册**：调用 `tools()`，每个工具注册到全局工具注册表
4. **保持**：库句柄在 `runtime.plugin_libraries` 中保持活跃，贯穿整个守护进程会话
5. **永不卸载**：库在守护进程会话期间永不卸载 -- 确保函数指针保持有效

这意味着：

- 你的插件的 `Drop` 实现（如果有）仅在守护进程关闭时运行
- 库中的静态状态在所有工具调用中持久化
- 可以安全使用 `lazy_static` 或 `once_cell` 进行初始化

### 步骤 6：本地测试

```bash
# 从开发目录安装
cortex plugin install ./my-plugin/

# 在 config.toml 中启用
# 将 "my-plugin" 添加到 [plugins].enabled

# 重启以加载
cortex restart

# 验证是否出现
cortex plugin list
```

### 步骤 7：打包和分发

```bash
# 创建 .cpx 归档
cortex plugin pack ./my-plugin/

# 其他人可以直接安装
cortex plugin install ./my-plugin.cpx
```

通过 GitHub 分发：

1. 为你的插件创建代码仓库
2. 构建并打包 `.cpx` 归档
3. 创建 GitHub release 并附加 `.cpx` 作为 release asset
4. 用户通过 `cortex plugin install owner/repo` 安装

---

## 完整插件示例

一个包含一个技能和一个原生工具的最小插件：

```
my-plugin/
  manifest.toml
  skills/
    my-skill/
      SKILL.md
  lib/
    libmy_plugin.so
```

**manifest.toml：**

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "Example plugin with one tool and one skill"
author = "Your Name"
cortex_version = "0.8"

[capabilities]
provides = ["tools", "skills"]
```

**skills/my-skill/SKILL.md：**

```markdown
---
description: Analyze code complexity metrics
when_to_use: When the user asks about code complexity or wants metrics
required_tools: [bash, read]
tags: [analysis, metrics]
activation:
  input_patterns:
    - "complexity"
    - "cyclomatic"
    - "code metrics"
---

# Complexity Analysis

1. Identify the target files or directory
2. Run static analysis tools to gather metrics
3. Summarize findings with actionable recommendations
```

**src/lib.rs：**

```rust
use cortex_sdk::prelude::*;
use serde_json::{json, Value};

#[derive(Default)]
pub struct MyPlugin;

impl MultiToolPlugin for MyPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "my-plugin".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "示例插件".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(MyTool)]
    }
}

struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn description(&self) -> &'static str { "Does something useful" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "The input to process" }
            },
            "required": ["input"]
        })
    }
    fn execute(&self, input: Value) -> Result<ToolResult, ToolError> {
        let text = input["input"].as_str().unwrap_or("");
        Ok(ToolResult::success(format!("Processed: {text}")))
    }
}

export_plugin!(MyPlugin);
```

---

## 版本兼容性

插件兼容性遵循以下规则：

- **主版本**：必须与运行中的 Cortex 主版本匹配
- **次版本**：Cortex 次版本必须 >= 插件声明的最低版本
- **修订版本**：兼容性检查中忽略

示例：声明 `cortex_version = "0.8"` 的插件适用于 Cortex 0.8.x 和 0.9.x，但不适用于 1.0.0。

如果插件加载时出现版本错误，请检查 `manifest.toml` 中的 `cortex_version` 与 `cortex --version` 输出是否匹配。

---

## 故障排除

### 插件未出现

1. 验证安装：`cortex plugin list` 应显示该插件
2. 检查是否在 `config.toml` 的 `[plugins].enabled` 中启用
3. 启用后重启守护进程
4. 检查日志中的加载错误：`journalctl --user -u cortex`

### 技能未激活

1. 验证技能文件位于插件目录内的 `skills/<name>/SKILL.md`
2. 检查 `activation.input_patterns` 是否与你的测试输入匹配
3. 确保 `required_tools` 在当前实例中全部可用
4. 检查 `cortex plugin list` 确认技能已注册

### 原生插件崩溃

1. 使用调试符号重建：`cargo build`（不加 --release）
2. 检查内存安全问题（空指针、释放后使用）
3. 验证 `cortex_plugin_create_multi` 函数返回有效的 `MultiToolPlugin`
4. 确保库在 Cargo.toml 中构建为 `cdylib`
5. 检查日志中的 dlopen 错误或符号解析失败

### 版本不匹配

1. 检查 `manifest.toml` 中的 `cortex_version` 字段
2. 运行 `cortex --version` 查看当前 Cortex 版本
3. 更新插件的 `cortex_version` 以匹配你的 major.minor
4. 重新构建并重新安装插件
