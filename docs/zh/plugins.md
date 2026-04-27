# 插件开发指南

Cortex 插件是可治理 package。插件不只是增加一个工具名；它必须声明 capability、effect、trust tier、sandbox 预期、package metadata 和 conformance 状态，让 runtime 和 operator 在使用前可以审阅。

本文描述两条公开插件边界：

- **进程 JSON**：进程隔离 JSON 工具的默认外部边界。Cortex 在每次工具调用时启动 manifest 声明的命令，通过 stdin 发送 JSON，并从 stdout 读取 JSON result。
- **强信任 native ABI**：低延迟进程内边界，用于基于 `cortex-sdk` 构建的本地代码。native 插件导出 `cortex_plugin_init`，返回 C 兼容函数表。Rust trait object 不跨动态库边界。

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

把 `bin/example-tool` 替换成你的实现。除非 operator 明确接受 `allow_host_paths = true`，否则 manifest 里的 command 路径必须保持在插件目录内。

## Manifest

每个插件都需要 `manifest.toml`。Manifest 是 package contract：身份、兼容性、capability request、sandbox profile、package metadata 和 tool declaration 都在这里声明。

```toml
name = "example"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.5.0"
trust = "reviewed_process"

[capabilities]
provides = ["tools", "skills"]
file_read = ["project/**"]
file_write = ["project/src/**"]
network = ["api.github.com"]
process = true
secrets = false
background = false

[sandbox]
level = "child_process"
network = "allowlist"
filesystem = "declared_paths"
writable_paths = ["project/src"]
seccomp = ""
uid_drop = false
memory_mb = 256
cpu_seconds = 10

[package]
publisher_id = "example.dev"
manifest_sha256 = ""
binary_sha256 = ""
signature = ""
sbom = "sbom.spdx.json"
risk_profile = "risk.toml"

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

[[native.tools.effects]]
kind = "write_file"
target = "project/src/**"
reversibility = "partially_reversible"
confirmation = "always"
dry_run = "supported"
```

规则：

- `cortex_version` 必填，并且会在 native library probe 前校验。
- `trust = "reviewed_process"` 是已审阅进程插件的正常层级。
- `trusted_in_process` native 插件必须使用 `trust = "trusted_native"`。
- `trust = "disabled"` 或 `trust = "quarantined"` 会阻止加载。
- 未审阅插件不能请求 `secrets = true`。
- `sandbox.network = "none"` 不能同时声明 network hosts。
- tool-level `effects` 会叠加 package-level capability effects，并进入风险评分。

## Capability Review

安装本地插件前先运行：

```bash
cortex plugin review ./cortex-plugin-example
```

命令形式：`cortex plugin review <dir>`。

Review 会展示：

- 请求的 file、network、process、secret、background capability；
- package signature 状态；
- conformance 状态；
- sandbox profile；
- 推荐的 `[risk.tools.<name>]` policy 行；
- 缺失 SBOM、缺失签名、缺失 conformance certificate 等治理警告。

目录安装也会在复制文件前打印同一份 review。

插件启用到实例后，运行 `cortex policy lint` 检查合并后的 config/plugin 姿态。它会发现已启用插件 manifest 缺失、open 权限模式下的 unreviewed plugin、native/process plugin 工具缺少显式 risk profile、secret access 没有确认/阻断策略，以及不安全的后台执行策略。

## Conformance Kit

插件作者和 operator 都可以运行：

```bash
cortex plugin test ./cortex-plugin-example
```

命令形式：`cortex plugin test <dir>`。

本地 conformance kit 会检查：

- manifest shape 和兼容性；
- governance 约束；
- process command 和 working directory 的路径边界；
- command 是否存在；
- timeout 和 output-limit 配置；
- secret-like environment inheritance；
- trusted native plugin 的 ABI 声明；
- declared capability/effect visibility。

失败的 conformance report 对 governed plugin package 来说是安装和发版阻断项。

## 进程 JSON 协议

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

当命令执行完成但工具结果应视为失败时，设置 `is_error = true`。如果进程非零退出且 stderr 有内容，Cortex 会把 stderr 作为工具错误返回；否则报告退出状态。stdout 不是合法 JSON 时会被拒绝。

## 打包

在插件目录执行：

```bash
cortex plugin test .
cortex plugin pack .
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx
```

也支持目录安装：

```bash
cortex plugin install ./cortex-plugin-example/
```

本地安装和 `.cpx` archive 只包含受支持资产：

- `manifest.toml`
- `package.toml`
- `sbom.spdx.json`
- `risk.toml`
- `conformance.toml`
- `lib/`
- `skills/`
- `prompts/`

隐藏条目、备份目录和无关文件会被忽略。如果 manifest 声明了 `[native].library` 但 `lib/` 缺失，Cortex 会自动把 `target/release/` 或 `target/debug/` 中的构建产物拷贝到安装后的 `lib/` 目录。

## 热重载

进程隔离命令实现更新会在下一次工具调用生效。manifest、schema 和 tool-set 变更会被 hot-reload watcher 检测到；Cortex 会卸载旧代理工具，并注册新的 manifest 声明工具。

安装或替换强信任 native 共享库时，需要重启 daemon 以加载新代码。强信任 native shared-library 代码变更仍需要 `cortex restart`，因为 daemon 会在进程生命周期内保持已加载共享库映射。

## 强信任 Native ABI

强信任 native 插件是基于 `cortex-sdk` 构建的共享库。它没有沙箱，只应在 operator 信任该代码可运行于 daemon 进程内时使用。

```toml
name = "dev"
version = "1.5.0"
description = "Trusted native development tools"
cortex_version = "1.5.0"
trust = "trusted_native"

[capabilities]
provides = ["tools", "skills"]

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_dev.so"
isolation = "trusted_in_process"
abi_version = 1
```

规则：

- native 插件必须导出 `cortex_plugin_init`。
- runtime 要求 `abi_version = 1`。
- `cortex_plugin_create`、`cortex_plugin_create_multi` 等旧符号会被拒绝。
- native 插件崩溃或未定义行为可能影响 daemon。

## Skills 和 Prompts

可选 Skills 放在 `skills/<skill-name>/SKILL.md`，可选 Prompt 片段放在 `prompts/`。它们会随插件打包，并跟随 plugin manifest 加载。
