# 插件开发指南

本文只描述 Cortex 当前受支持的插件路径：在 `manifest.toml` 中声明进程隔离工具，并通过 JSON stdin/stdout 协议调用。

## 概述

Cortex 插件可以贡献工具、Skills、Prompt 文件和结构化媒体附件，不依赖 Cortex 内部 crate。工具执行采用进程隔离：Cortex 在每次工具调用时启动 manifest 声明的命令，把 JSON request 写入 stdin，并从 stdout 读取 JSON result。

进程 JSON 协议是唯一文档化的插件执行边界。它支持 manifest、schema、tool-set 热重载，让插件命令运行在 daemon 进程外，并避免 Rust trait-object ABI 耦合。

## 脚手架

```bash
cortex --new-process-plugin example
cd cortex-plugin-example
```

脚手架生成：

```text
cortex-plugin-example/
├── manifest.toml
├── bin/
│   └── example-tool
├── skills/
├── prompts/
└── README.md
```

把 `bin/example-tool` 替换为你的实现。除非显式设置 `allow_host_paths = true`，否则 manifest command 路径应保持在插件目录内。

## Manifest

每个插件都需要 `manifest.toml`：

```toml
name = "example"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools", "skills"]

[native]
isolation = "process"

[[native.tools]]
name = "example"
description = "Example process-isolated tool"
command = "bin/example-tool"
args = []
working_dir = "."
inherit_env = ["PATH"]
env = { CORTEX_PLUGIN_MODE = "isolated" }
timeout_secs = 5
max_output_bytes = 1048576
max_memory_bytes = 67108864
max_cpu_secs = 2
input_schema = { type = "object", properties = { input = { type = "string" } }, required = ["input"] }
```

规则：

- `cortex_version` 必填。
- 文档化插件的 `[native].isolation` 必须是 `"process"`。
- `command` 和 `working_dir` 默认相对插件目录解析。
- 绝对宿主路径默认拒绝，除非设置 `allow_host_paths = true`。
- 进程环境会先清空，再应用 `inherit_env` 和 `env`。
- `timeout_secs`、`max_output_bytes`、`max_memory_bytes`、`max_cpu_secs` 约束每次调用。

## 协议

Cortex 向 stdin 写入一条 JSON request：

```json
{"tool":"example","input":{"input":"hello"}}
```

工具可以返回 JSON string：

```json
"Processed: hello"
```

也可以返回 object：

```json
{"output":"Processed: hello","is_error":false}
```

当命令执行完成但工具结果应视为失败时，设置 `is_error = true`。

## 打包

在插件目录执行：

```bash
cortex plugin pack .
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx
cortex restart
```

## 热重载

进程隔离命令实现更新会在下一次工具调用生效。manifest、schema 和 tool-set 变更会被 hot-reload watcher 检测到；Cortex 会卸载该插件旧代理工具，并注册新的 manifest 声明工具。

## Skills 和 Prompts

可选 Skills 放在 `skills/<skill-name>/SKILL.md`，可选 Prompt 片段放在 `prompts/`。它们会随插件打包，并跟随 plugin manifest 加载。
