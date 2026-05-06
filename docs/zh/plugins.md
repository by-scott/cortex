# 插件开发指南

Cortex 插件是可治理 package。插件不只是一个工具名，而是一个可签名、可审阅的 runtime extension；它必须声明身份、最低支持 Cortex 版本、capability、effect、trust tier、sandbox 姿态、package metadata 和 conformance 状态。

本文从第一条命令讲到发布 package，覆盖两条插件边界：

- **进程 JSON**：进程隔离 JSON 工具的默认边界。Cortex 每次工具调用都会启动 manifest 声明的命令，通过 stdin 发送一条 JSON request，并从 stdout 读取一条 JSON result。
- **强信任 native ABI**：基于 `cortex-sdk` 构建的本地 Rust 共享库边界。native 插件导出 `cortex_plugin_init`，返回 C 兼容函数表。Rust trait object 不跨动态库边界。

完整参考 package 可以看 [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev)。它是官方开发插件，用来把代码和项目维护工作流留在 daemon core 之外，同时完整走一遍第三方插件也必须遵守的 manifest、SDK、签名、effect、permission 和 protected-root 契约。

## 开始前

你需要：

- 可用的 `cortex` 二进制，推荐来自当前 GitHub Release。
- 如果要通过 `owner/repo` 分发插件，需要一个 GitHub 仓库。
- 一个 publisher id，例如 `example.dev` 或 `by-scott`。
- 一个保存在仓库外的签名私钥。

这些文件默认不要提交到 git：

```text
package.toml
*.cpx
*.cpx.sha256
*.ed25519
```

`package.toml` 由签名命令生成，`.cpx` 是发布产物，私钥不能进入仓库。

## 选择边界

| 边界 | 适合场景 | 权衡 |
|------|----------|------|
| 进程 JSON | 大多数第三方工具、Shell/Python/Node/Rust 命令、跨语言插件 | 更清晰的进程边界，每次工具调用启动一次进程 |
| 强信任 native ABI | 需要低延迟或 runtime callback 的本地可信 Rust 工具 | 运行在 daemon 进程内，可能影响 daemon 稳定性 |

不确定时，先用进程 JSON。只有当插件代码被信任到可以运行在 daemon 进程内时，才使用强信任 native。

## 路径一：从脚手架到发布进程插件

### 1. 创建脚手架

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

把 `bin/example-tool` 替换成你的实现。除非 operator 明确接受 `allow_host_paths = true`，否则命令路径要保持在插件目录内。

### 2. 理解 Manifest

每个插件都需要 `manifest.toml`。Manifest 是 package contract：身份、最低支持 Cortex 版本、capability request、sandbox profile、package metadata 和 tool declaration 都在这里声明。

```toml
name = "example"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.6.0"
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
signature_algorithm = ""
public_key = ""
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

- `cortex_version` 必填，含义是最低支持的 Cortex runtime 版本，并且会在 native library probe 前校验。像 `1.6.0` 这样的具体版本可以在更新的兼容 Cortex（例如 `1.6.6`）上继续使用；version range 会被拒绝。这里应声明插件实际支持的最老 Cortex runtime 版本，而不是构建插件时碰巧使用的 Cortex 发布版本。
- `trust = "reviewed_process"` 是已审阅进程插件的正常层级。
- `trusted_in_process` native 插件必须使用 `trust = "trusted_native"`。
- `trust = "disabled"` 或 `trust = "quarantined"` 会阻止加载。
- 未审阅插件不能请求 `secrets = true`。
- Runtime 会拒绝当前无法落实的 sandbox enforcement 声明：`sandbox.level = "uid_no_network"`、`system_sandbox`、`container_vm`、`remote_worker`、`sandbox.network = "none"`、非空 `sandbox.seccomp`、以及 `sandbox.uid_drop = true`。
- tool-level `effects` 会叠加 package-level capability effects，并进入风险评分。

### 3. 实现进程协议

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

### 4. 审阅和测试

```bash
cortex plugin review .
cortex plugin test .
```

命令形式：

```text
cortex plugin review <dir>
cortex plugin test <dir>
```

`review` 会展示请求的 file、network、process、secret、background capability，package signature 状态，conformance 状态，sandbox profile，推荐的 `[risk.tools.<name>]` policy 行，以及缺失 SBOM、缺失签名、缺失 conformance certificate 等治理警告。

`test` 会运行本地 conformance kit，检查 manifest shape、最低支持 Cortex 版本、governance 约束、process command 和 working directory 边界、command 是否存在、timeout 和 output-limit 值、secret-like environment inheritance、trusted native plugin 的 ABI 声明、package capability visibility、per-tool effect visibility，以及进程隔离工具强制暴露的 `RunProcess:plugin subprocess` effect。

发布或外部审查时，填写[插件 Conformance 模板](plugin-conformance-template.md)。该模板记录 invalid JSON、stderr/non-zero exit、path escape、environment inheritance、output-limit、timeout、process-underreporting、不支持的 sandbox claim、native ABI mismatch 和 package filtering 等向量的本地证据。它是 operator review artifact，不是独立认证机构，也不是沙箱隔离声明。

对于 trusted native plugin，manifest capabilities 是包级信任边界，不会被复制到每一个工具上。LLM permission check 使用每个 native tool descriptor 自己声明的 effects，这样只读工具不会因为插件包整体具备 process 能力而被误判成脚本逃逸；真正的 process/write/network 工具仍然必须逐个声明自己的 effects。process-isolated tool 不同：工具调用本身就是启动子进程，所以 host 始终会补上 `RunProcess:plugin subprocess`。

失败的 conformance report 对 governed package 来说是安装和发版阻断项。

插件不能把 Prompt、配置、会话、Journal 或记忆修改实现成模型可直接调用的捷径。自我演化类插件可以分析证据并返回结构化 proposal，但应用 proposal 必须交给受检查的 PromptManager/runtime command 路径，由它负责 layer scope、lint、backup、原子写入和审计记录。

### 5. 签名和打包

只需要创建一次 publisher key：

```bash
mkdir -p ~/.config/cortex/plugin-signing
cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
```

每次发布前签名并打包：

```bash
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
sha256sum cortex-plugin-example-v0.1.0-linux-amd64.cpx > cortex-plugin-example-v0.1.0-linux-amd64.cpx.sha256
```

`cortex plugin sign` 会写入 `package.toml`。`cortex plugin pack` 会把 `package.toml` 和受支持的插件资产一起打包：

- `manifest.toml`
- `package.toml`
- `sbom.spdx.json`
- `risk.toml`
- `conformance.toml`
- `lib/`
- `skills/`
- `prompts/`

隐藏条目、备份目录、符号链接和无关文件会被忽略。

### 6. 发布

创建 GitHub Release，通常命名为 `v0.1.0`，并上传：

```text
cortex-plugin-example-v0.1.0-linux-amd64.cpx
cortex-plugin-example-v0.1.0-linux-amd64.cpx.sha256
```

用户安装最新 release：

```bash
cortex plugin install owner/cortex-plugin-example
```

或安装指定版本：

```bash
cortex plugin install owner/cortex-plugin-example@0.1.0
```

只有在审阅过已验证的 publisher key fingerprint 后，才使用 `--yes`：

```bash
cortex plugin install owner/cortex-plugin-example --yes
```

`--yes` 不会绕过签名、hash、manifest、package 或 archive safety 检查；它只是在安装模式无法交互询问时，记录对已验签 publisher key 的本机信任。

## 路径二：使用 `cortex-sdk` 构建强信任 Native 插件

SDK 发布节奏独立于 Cortex runtime 发布。插件应使用满足其 native ABI/DTO
需求的最新 `cortex-sdk`，但不要因为 Cortex 本身发布了 patch 就跟着 bump SDK
依赖。运行时兼容性由 `manifest.toml` 里的 `cortex_version` 决定：只要正在运行的
Cortex 版本大于等于插件声明的最低版本，并且 native ABI version 匹配，插件就可以加载。

### 1. 创建 crate

```bash
cargo new --lib cortex-plugin-native-hello
cd cortex-plugin-native-hello
```

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
cortex-sdk = "1.6.4"
serde_json = "1"
```

### 2. 实现 Native ABI

`src/lib.rs`：

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct NativeHelloPlugin;

impl MultiToolPlugin for NativeHelloPlugin {
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
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(NativeHelloPlugin);
```

### 3. 添加 Manifest

```toml
name = "native-hello"
version = "0.1.0"
description = "Example trusted native Cortex plugin"
cortex_version = "1.6.0"
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

Runtime 要求 `abi_version = 1`。native 插件必须导出 `cortex_plugin_init`。`cortex_plugin_create`、`cortex_plugin_create_multi` 等旧符号会被拒绝。

### 4. 构建、审阅、测试

```bash
cargo build --release
cortex plugin review .
cortex plugin test .
```

如果 `lib/` 缺失但 `[native].library` 已声明，Cortex 会自动把 `target/release/` 或 `target/debug/` 中的共享库复制进安装或打包后的插件目录。

### 5. 签名、打包、发布

```bash
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
sha256sum cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx.sha256
```

把 `.cpx` 和 `.sha256` 上传到 GitHub Release。用户安装并重启：

```bash
cortex plugin install owner/cortex-plugin-native-hello@0.1.0
cortex restart
```

安装或替换强信任 native 共享库时，需要重启 daemon，因为 daemon 会在进程生命周期内保持已加载共享库映射。

## 签名发布与信任

打包安装使用本机信任链治理：

1. 发布者用 `cortex plugin keygen` 创建 Ed25519 签名私钥。
2. 发布者在打包前运行 `cortex plugin sign`。该命令会写入 `package.toml`，包含 publisher id、public key、signature algorithm、manifest hash、native artifact hash、SBOM/risk/conformance 引用和 package signature。
3. `cortex plugin pack` 会把 `package.toml` 和受支持的插件资产一起打进 `.cpx`。
4. 用户安装 `.cpx`、URL 或 GitHub release package 时，Cortex 会先验签，再接受文件。

验签覆盖的是签名 payload，不只是 package 名称。Cortex 会检查：

- `manifest.toml` 以及 package metadata 中记录的 native library hash；
- `lib/`、`skills/`、`prompts/` 下所有受支持文件，以及存在时的 SBOM、risk profile、conformance 文件；
- Ed25519 signature 是否能用声明的 public key 验证；
- publisher key fingerprint 是否已存在于 `$CORTEX_HOME/plugin-trust.toml` 本机信任库。

以下情况会拒绝安装：打包安装策略下缺少签名、签名后任一受签文件被修改、signature 与 public key 不匹配、manifest/native hash 不一致，或 publisher key 未被信任且当前安装模式无法交互确认或显式信任。

当前版本没有中心 registry 或吊销服务。信任是本机状态，应像接受 SSH host key 一样处理：签名必须先在数学上验证通过，然后由 operator 决定是否信任该 publisher key 的后续 package。

## 热重载

进程隔离命令实现更新会在下一次工具调用生效。manifest、schema 和 tool-set 变更会被 hot-reload watcher 检测到；Cortex 会卸载旧代理工具，并注册新的 manifest 声明工具。

安装或替换强信任 native 共享库时，需要重启 daemon 以加载新代码。强信任 native shared-library 代码变更仍需要 `cortex restart`，因为 daemon 会在进程生命周期内保持已加载共享库映射。

## Skills 和 Prompts

可选 Skills 放在 `skills/<skill-name>/SKILL.md`，可选 Prompt 片段放在 `prompts/`。它们会随插件打包，并跟随 plugin manifest 加载。
