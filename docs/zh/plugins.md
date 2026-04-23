# 插件开发指南

本文描述 Cortex 的两个公开插件边界：进程隔离 JSON 工具，以及强信任 native ABI 工具。

## 概述

Cortex 插件可以贡献工具、Skills、Prompt 文件和结构化媒体附件，不依赖 Cortex 内部 crate。

进程 JSON 是默认边界：Cortex 在每次工具调用时启动 manifest 声明的命令，把 JSON request 写入 stdin，并从 stdout 读取 JSON result。

强信任 native ABI 是低延迟边界，用于必须在 daemon 进程内运行的本地插件。native 插件导出 `cortex_plugin_init`，返回 C 兼容函数表。Cortex 不加载 Rust trait-object 符号。

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

## 进程 JSON Manifest

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

## 强信任 Native ABI

强信任 native 插件是基于 `cortex-sdk` 构建的共享库。它不提供沙箱隔离，代码变更后需要重启 daemon。

```toml
name = "dev"
version = "1.2.0"
description = "Trusted native development tools"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools", "skills"]

[native]
library = "lib/libcortex_plugin_dev.so"
isolation = "trusted_in_process"
abi_version = 1
```

规则：

- native 插件必须导出 `cortex_plugin_init`。
- runtime 要求 `abi_version = 1`。
- `cortex_plugin_create`、`cortex_plugin_create_multi` 等旧符号会被拒绝。
- native 插件是强信任扩展：崩溃或未定义行为可能影响 daemon。

## Skills 和 Prompts

可选 Skills 放在 `skills/<skill-name>/SKILL.md`，可选 Prompt 片段放在 `prompts/`。它们会随插件打包，并跟随 plugin manifest 加载。
