# Cortex插件开发指南

语言：[English](../plugin-development.md) | 简体中文

本文假设你从未开发过Cortex插件。它会先解释基本概念，再从最小进程隔离插件开始，
逐步走到manifest、工具协议、skills、审查、安装、签名、打包、发布，以及可信native插件。

相关文档：[README](../../README.zh.md)、[使用指南](usage.md)

## 什么是Cortex插件

Cortex插件本质上是一个根目录包含`manifest.toml`的目录。Manifest声明插件是谁、
需要哪个Cortex版本、请求哪些能力，以及Cortex应该如何加载它。

插件可以提供三类内容：

- **Tools**：模型在turn中可以调用的动作。
- **Skills**：Cortex可以按需注入上下文或触发的`SKILL.md`操作流程。
- **Prompts**：随插件一起分发的prompt fragments。

工具有两种执行边界：

- **进程隔离工具**：Cortex每次工具调用启动一个子进程，通过stdin发送JSON，通过stdout读取JSON。这是新插件的默认起点。工具可以用shell、Python、Rust、Go、Node.js或任何可执行程序编写。
- **可信native插件**：Cortex通过`cortex-sdk`把Rust shared library加载进daemon进程。只有当你需要进程内性能或SDK深度集成，并且愿意把插件当作可信代码治理时才使用。

第一个插件请从进程隔离插件开始。它不要求你会Rust。

## 开始前准备

先确认Cortex可用：

```sh
cortex doctor
cortex plugin list
```

你需要：

- 一个shell。
- 可工作的`cortex`二进制。
- 一个可以创建插件项目的目录。
- 只有开发可信native插件时才需要Rust。

基本术语：

- **源码目录**：你的插件项目目录。
- **已安装插件目录**：Cortex插件存储中的副本。
- **`.cpx` package**：`cortex plugin pack`生成的插件包格式。
- **Publisher key**：用于签名package metadata的Ed25519私钥。
- **SBOM**：software bill of materials，通常是`sbom.spdx.json`。
- **Risk profile**：package governance metadata，通常是`risk.toml`。

## 构建第一个插件

创建进程插件脚手架：

```sh
cortex --new-process-plugin hello
cd cortex-plugin-hello
```

脚手架会生成：

```text
cortex-plugin-hello/
├── README.md
├── bin/
│   └── hello-tool
├── manifest.toml
├── prompts/
└── skills/
```

这些文件的含义：

- `manifest.toml`是运行时契约。Cortex会拒绝格式错误或不安全的manifest。
- `bin/hello-tool`是可执行的进程工具。
- `skills/`存放插件skills。
- `prompts/`存放随包分发的prompt fragments。
- `README.md`是插件项目自己的说明文档。

审查生成的插件：

```sh
cortex plugin review .
```

运行本地conformance checks：

```sh
cortex plugin test .
```

安装到当前Cortex实例：

```sh
cortex plugin install .
```

目录安装属于开发者安装：Cortex会审查目录、复制到插件存储，并为当前实例启用该插件。
如果daemon正在运行，进程工具可见性会很快热加载。不确定时重启：

```sh
cortex restart
cortex plugin list
```

在Cortex中试用：

```sh
cortex "Use the hello tool with input 'world' and tell me the result."
```

模型会决定是否调用工具。清晰的工具名、描述和schema会显著提高调用可靠性。

## 进程工具协议

每次进程工具调用时，Cortex会启动manifest中声明的命令，并向stdin写入一个JSON object：

```json
{
  "tool": "hello",
  "input": {
    "input": "world"
  }
}
```

其中`input` object必须符合`manifest.toml`里的`input_schema`。

工具必须向stdout写入一个JSON value。最简单的合法输出是JSON string：

```json
"hello world"
```

推荐输出object：

```json
{
  "output": "hello world",
  "is_error": false
}
```

规则：

- `output`必须是string。
- `is_error = true`表示可恢复工具错误，模型可见。
- 子进程非零退出会变成工具错误。如果stderr有内容，Cortex使用stderr作为错误信息。
- 成功时stdout必须是合法JSON。
- stdout加stderr受`max_output_bytes`限制。
- 达到`timeout_secs`后，Cortex会kill子进程并返回timeout error。

脚手架生成的shell脚本只是最小示例。生产插件应使用所选语言的真实JSON parser，不要用正则解析JSON。

## 从零理解Manifest

`manifest.toml`是严格格式：未知字段会被拒绝。

最小进程插件：

```toml
name = "hello"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
author = "example.dev"
cortex_version = "1.6.15"
trust = "reviewed_process"

[capabilities]
provides = ["tools", "skills"]
file_read = []
file_write = []
network = []
process = false
secrets = false
background = false

[sandbox]
level = "child_process"
network = "inherit"
filesystem = "plugin_only"

[native]
isolation = "process"

[[native.tools]]
name = "hello"
description = "Return a short greeting for the supplied input."
command = "bin/hello-tool"
inherit_env = ["PATH"]
timeout_secs = 5
max_output_bytes = 1048576
max_memory_bytes = 67108864
max_cpu_secs = 2
input_schema = { type = "object", properties = { input = { type = "string" } }, required = ["input"] }
```

身份字段：

- `name`：插件名称。保持短、稳定。
- `version`：插件版本。
- `description`：一句话说明用途。
- `author`：publisher或维护者身份。
- `cortex_version`：插件需要的最低Cortex runtime版本。Cortex接受小于或等于当前runtime的具体版本，拒绝版本范围。
- `trust`：治理等级。进程插件通常是`reviewed_process`，native插件通常是`trusted_native`。

能力字段：

- `provides`：声明package提供什么，例如`tools`、`skills`或`prompts`。
- `file_read`：插件工具请求读取的文件glob。
- `file_write`：插件工具请求写入的文件glob。
- `network`：插件工具请求访问的网络host。
- `process`：插件工具是否会自行启动子进程。
- `secrets`：插件是否请求host secrets或继承凭据。
- `background`：插件是否请求后台或长时间运行。

能力声明要真实且最小。这些字段会进入review、policy和operator trust决策。

进程工具字段：

- `name`：注册到Cortex的工具名。使用小写和下划线。
- `description`：写给模型看。说明工具做什么、何时使用、何时不该使用。
- `command`：可执行文件路径。相对路径会在插件目录内解析。
- `args`：可选命令参数。
- `working_dir`：可选工作目录。相对路径会在插件目录内解析。
- `allow_host_paths`：默认false。除非操作者明确信任host-level path，否则保持false。
- `inherit_env`：继承环境变量allowlist。为空时Cortex提供包含`PATH`的最小默认环境。
- `env`：显式设置给工具的环境变量。
- `timeout_secs`：子进程硬超时。
- `max_output_bytes`：stdout加stderr最大字节数。
- `max_memory_bytes`：Unix virtual memory限制。
- `max_cpu_secs`：Unix CPU秒数限制。
- `input_schema`：工具输入的JSON Schema object。
- `effects`：可选工具级副作用。插件级capability effects始终作为下限附加。

## JSON Schema基础

模型用`input_schema`判断工具输入形状。Schema要小而精确。

一个必填string：

```toml
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

String enum：

```toml
input_schema = { type = "object", properties = { format = { type = "string", enum = ["text", "json"] } } }
```

两个必填字段：

```toml
input_schema = { type = "object", properties = { path = { type = "string" }, query = { type = "string" } }, required = ["path", "query"] }
```

好的schema会减少模型生成错误参数。坏schema会让工具调用变得嘈杂。

## 添加一个真实工具

添加第二个可执行文件：

```sh
mkdir -p bin
cat > bin/reverse-text <<'SH'
# !/bin/sh
set -eu
request=$(cat)
text=$(printf '%s' "$request" | sed -n 's/.*"text"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
output=$(printf '%s' "$text" | rev)
printf '{"output":"%s","is_error":false}\n' "$output"
SH
chmod +x bin/reverse-text
```

声明工具：

```toml
[[native.tools]]
name = "reverse_text"
description = "Reverse a short text string. Use for simple text reversal requests."
command = "bin/reverse-text"
timeout_secs = 5
max_output_bytes = 65536
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

然后运行：

```sh
cortex plugin review .
cortex plugin test .
cortex plugin install .
cortex restart
```

让Cortex使用它：

```sh
cortex "Use the reverse_text tool to reverse 'cortex'."
```

## 添加Skill

Skills是带YAML frontmatter的markdown文件，路径是`skills/<name>/SKILL.md`。

```text
skills/refactor-small/
└── SKILL.md
```

示例：

```markdown
---
name: refactor-small
description: Refactor a small code region while preserving behavior.
when_to_use: Use when the user asks for a narrow cleanup or simplification.
required_tools:
  - bash
execution_mode: Inline
timeout_secs: 600
tags:
  - refactor
  - quality
user_invocable: true
agent_invocable: true
version: 0.1.0
activation:
  input_patterns:
    - (?i)(refactor|cleanup|simplify)
---

# Refactor Small

${ARGS}

## Steps

1. Locate the owning module and read nearby code.
2. Make the smallest behavior-preserving edit.
3. Run the relevant check.
4. Report what changed and what was verified.
```

支持的frontmatter字段：

- `name`：必填。
- `description`：必填。
- `when_to_use`：可选路由说明。
- `parameters`：可选`{ name, description, required }`列表。
- `required_tools`：该skill预期使用的运行时工具列表。
- `execution_mode`：`Inline`或`Fork`。
- `timeout_secs`：可选预期超时。
- `tags`：可选标签列表。
- `user_invocable`：默认true。
- `agent_invocable`：默认true。
- `version`：可选skill版本。
- `activation.input_patterns`：匹配用户输入的regex。
- `activation.pressure_above`：可选上下文压力触发条件。
- `activation.alert_kinds`：可选元认知告警触发列表。
- `activation.event_kinds`：可选运行时事件触发列表。

`${ARGS}`会在skill渲染时替换为调用参数。

## Review与Conformance

安装或打包前运行review：

```sh
cortex plugin review .
```

Review会显示：

- plugin identity
- trust tier
- package signature state
- conformance state
- requested capabilities
- recommended risk profile
- warnings
- checks

运行conformance：

```sh
cortex plugin test .
```

Conformance checks包括manifest identity、version、Cortex version target、capability declaration、
native isolation boundary、process tool path safety、command existence、output limit、timeout、
working directory和inherited environment risks。

## Package Metadata

可发布插件应携带治理metadata：

- `package.toml`：publisher id、public key、hashes、signature、SBOM path、risk profile path，以及可选conformance certificate。
- `risk.toml`：人类可读的package risk profile。
- `sbom.spdx.json`：SPDX JSON格式的软件物料清单。
- `conformance.toml`：如果你的发布流程会写入conformance evidence，可包含该文件。

`cortex plugin sign`会写入`package.toml`。它不会替你生成私钥、SBOM或外部发布流程。
签名key必须保存在仓库外。

## 签名与打包

创建本地Ed25519 signing key：

```sh
cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
```

签名插件：

```sh
cortex plugin sign . \
  --key ~/.config/cortex/plugin-signing/example-dev.ed25519 \
  --publisher example.dev
```

打包：

```sh
cortex plugin pack .
```

默认archive名称：

```text
<directory>-v<version>-<platform>.cpx
```

Archive可包含：

- `manifest.toml`
- `package.toml`
- `sbom.spdx.json`
- `risk.toml`
- `conformance.toml`
- `lib/`
- `skills/`
- `prompts/`

应在package内容最终确定后签名。如果签名后修改manifest、skills、prompts或native library，
需要重新签名。

## 安装来源

从本地package安装：

```sh
cortex plugin install ./cortex-plugin-hello-v0.1.0-linux-amd64.cpx
cortex restart
cortex plugin list
```

本地开发时从目录安装：

```sh
cortex plugin install .
cortex restart
```

按仓库名和版本安装：

```sh
cortex plugin install by-scott/cortex-plugin-dev@1.6.10
```

不带owner的名称会解析为GitHub仓库`by-scott/cortex-plugin-<name>`。

## 可信Native插件

当插件必须作为可信进程内Rust代码运行时，使用`cortex-sdk`。Native插件编译为`cdylib`
shared library，并通过`cortex_sdk::export_plugin!`导出稳定`cortex_plugin_init` ABI。

除非你明确知道为什么需要native，否则先使用进程插件。

`Cargo.toml`：

```toml
[package]
name = "cortex-plugin-native-hello"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cortex-sdk = "1.6.15"
serde_json = "1"
```

`src/lib.rs`：

```rust
use cortex_sdk::prelude::*;

# [derive(Default)]
struct NativeHello;

impl MultiToolPlugin for NativeHello {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "native-hello".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example trusted native Cortex plugin".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(WordCountTool)]
    }
}

struct WordCountTool;

impl Tool for WordCountTool {
    fn name(&self) -> &'static str {
        "word_count"
    }

    fn description(&self) -> &'static str {
        "Count words in a text string."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(NativeHello);
```

Native manifest：

```toml
name = "native-hello"
version = "0.1.0"
description = "Example trusted native Cortex plugin"
author = "example.dev"
cortex_version = "1.6.15"
trust = "trusted_native"

[capabilities]
provides = ["tools"]
secrets = false

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_native_hello.so"
isolation = "trusted_in_process"
abi_version = 1
```

构建与打包：

```sh
cargo build --release
mkdir -p lib
cp target/release/libcortex_plugin_native_hello.so lib/
cortex plugin review .
cortex plugin test .
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
```

Native插件更新后需要`cortex restart`。

## 发布

将`.cpx` package作为GitHub Release asset发布。Release应包含package和由发布流程生成的checksum文件：

```sh
sha256sum cortex-plugin-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-hello-v0.1.0-linux-amd64.cpx.sha256
```

推荐发布纪律：

- 保持`manifest.toml`、`package.toml`、`sbom.spdx.json`和`risk.toml`最新。
- 签名前运行`cortex plugin review .`和`cortex plugin test .`。
- 只有在artifact内容最终确定后才签名。
- 不提交私钥、本地secret、生成的`target/`目录或临时archive。
- 只有当插件真的依赖新的runtime contract时，才提升`cortex_version`。

## 故障排查

| 现象 | 常见原因 | 修复 |
| --- | --- | --- |
| `manifest.toml` missing | 命令不在插件根目录运行 | `cd`到插件目录 |
| `invalid manifest.toml` | 字段未知或拼写错误 | 对照本文manifest字段检查 |
| command path review失败 | 工具命令逃逸插件目录或文件缺失 | 把可执行文件放到`bin/`，保持`allow_host_paths = false` |
| command is not executable | 缺少executable bit | `chmod +x bin/<tool>` |
| invalid JSON output | 工具向stdout打印了日志或纯文本 | stdout只打印JSON，日志写stderr |
| 插件已安装但工具没被用 | 描述/schema不清晰，或daemon尚未reload | 改进工具描述，运行`cortex plugin list`，重启daemon |
| signature invalid | 签名后又改了文件 | 重新review、test、sign、pack |

## 设计检查清单

发布前逐项确认：

- 插件名短且稳定。
- `cortex_version`是实际需要的最低runtime contract。
- 除非native有明确理由，否则使用进程隔离边界。
- 每个工具都有窄JSON Schema。
- 每个工具描述都告诉模型何时使用。
- Capabilities真实且最小。
- 除非声明且必要，否则不继承secrets。
- 工具stdout是合法JSON。
- `cortex plugin review .`达到可发布状态。
- `cortex plugin test .`通过。
- 签名key在仓库外。
- 最终artifact就位后再签名。
