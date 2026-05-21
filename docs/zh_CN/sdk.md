# Cortex SDK指南

[English](../sdk.md) · [简体中文](sdk.md)

本文介绍可信native Cortex插件使用的Rust SDK。进程隔离JSON插件不需要
`cortex-sdk`，应先阅读[插件开发指南](plugin-development.md)。

## SDK提供什么

`cortex-sdk`是native插件的稳定Rust边界：

- `MultiToolPlugin`描述插件元数据并创建工具。
- `Tool`描述可被模型调用的工具。
- `ToolResult`和`ToolError`定义工具输出。
- `InvocationContext`标识session、actor、source和执行范围。
- `ToolRuntime`让工具发送进度和observer文本。
- `ToolEffect`和`ToolCapabilities`描述副作用，供policy gate使用。
- `Attachment`返回图片、音频、视频或文件输出。
- `export_plugin!`暴露native ABI入口。

SDK不依赖Cortex内部crate。插件通常只依赖`cortex-sdk`、`serde`、
`serde_json`和自身实现所需的库。

## 项目初始化

创建Rust library crate：

```sh
cargo new cortex-plugin-example --lib
cd cortex-plugin-example
```

配置`Cargo.toml`：

```toml
[package]
name = "cortex-plugin-example"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cortex-sdk = "1.7.0"
serde_json = "1"
```

`cdylib`用于让daemon加载共享库，`rlib`方便本地检查和编辑器分析。

## 实现工具

```rust
use cortex_sdk::prelude::*;

# [derive(Default)]
struct ExamplePlugin;

impl MultiToolPlugin for ExamplePlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "example".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example Cortex native plugin".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(WordCount)]
    }
}

struct WordCount;

impl Tool for WordCount {
    fn name(&self) -> &'static str {
        "word_count"
    }

    fn description(&self) -> &'static str {
        "Count words in provided text."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to count"
                }
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

cortex_sdk::export_plugin!(ExamplePlugin);
```

工具描述会被模型读取。描述应说明工具做什么、何时使用、需要什么输入。

## 使用运行时上下文

当工具需要session元数据、actor、source、执行范围、进度事件或observer文本时，
实现`execute_with_runtime`：

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    ctx: InvocationContext,
    runtime: &dyn ToolRuntime,
) -> Result<ToolResult, ToolError> {
    runtime.progress("analyzing input");
    runtime.observer_text("example", &format!("source={}", ctx.source));
    self.execute(input)
}
```

运行时事件用于可观测性。返回的`ToolResult`仍然是传回模型的权威输出。

## 声明副作用

副作用声明让Cortex在工具运行前执行风险策略：

```rust
fn effects(&self, input: &serde_json::Value) -> Vec<ToolEffect> {
    let path = input["path"].as_str().unwrap_or_default();
    vec![ToolEffect::file_read(path)]
}
```

文件系统、进程、网络或状态变更都应明确声明。Policy只能基于工具声明的实际影响面来保护用户。

## 返回媒体

生成文件应通过`Attachment`返回：

```rust
Ok(ToolResult::success("image ready").with_media(Attachment {
    media_type: "image".into(),
    mime_type: "image/png".into(),
    url: "/tmp/example.png".into(),
    caption: Some("Example output".into()),
    size: None,
}))
```

插件不应直接调用Telegram、QQ、HTTP或其它transport API。Cortex会根据当前渠道发送媒体。

## Manifest

在插件根目录创建`manifest.toml`：

```toml
name = "example"
version = "0.1.0"
description = "Example Cortex native plugin"
cortex_version = "1.7.0"
trust = "trusted_native"

[capabilities]
provides = ["tools"]
file_read = []
file_write = []
network = []
process = false
secrets = false
background = false

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_example.so"
isolation = "trusted_in_process"
abi_version = 1
```

`cortex_version`是插件支持的最低Cortex runtime版本。`abi_version`必须匹配SDK的native ABI。

## 构建与打包

```sh
cargo build --release
mkdir -p lib
cp target/release/libcortex_plugin_example.so lib/

cortex plugin review .
cortex plugin test .
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example.ed25519 --publisher example
cortex plugin pack .
```

签名私钥必须保存在仓库之外。

## API参考

完整API参考见docs.rs：

<https://docs.rs/cortex-sdk/latest/cortex_sdk/>
