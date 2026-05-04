# 本地模型

本文说明首次使用 Cortex 时的本地模型配置。它只是配置指南：`cortex doctor` 会根据配置报告本地端点线索，但除非未来明确加入并记录主动检查，否则它不会主动证明 provider 可达。

## Ollama

`cortex demo` 已经写入偏向 Ollama 的 demo 配置：

```bash
cortex demo
ollama pull qwen2.5-coder:7b
cortex doctor --id demo
cortex policy lint --id demo
```

生成的实例使用：

```toml
[api]
provider = "ollama"
api_key = ""
model = "qwen2.5-coder:7b"
preset = "minimal"

[embedding]
provider = "ollama"
model = "nomic-embed-text"
```

默认 `providers.toml` 包含：

```toml
[ollama]
name = "Ollama"
protocol = "ollama"
base_url = "http://localhost:11434"
auth_type = "none"
models = []
```

用 Ollama 自己的端点验证服务：

```bash
curl http://localhost:11434/api/tags
```

## vLLM

先用 vLLM 启动 OpenAI 兼容服务，然后增加 provider 条目：

```toml
[vllm-local]
name = "vLLM Local"
protocol = "openai"
base_url = "http://127.0.0.1:8000/v1"
auth_type = "none"
models = ["Qwen/Qwen2.5-Coder-7B-Instruct"]
```

再把实例配置指向该 provider：

```toml
[api]
provider = "vllm-local"
api_key = ""
model = "Qwen/Qwen2.5-Coder-7B-Instruct"
preset = "minimal"
```

直接检查 vLLM 端点：

```bash
curl http://127.0.0.1:8000/v1/models
cortex doctor --id demo
cortex policy lint --id demo
```

如果 vLLM 服务要求 bearer auth，在 `providers.toml` 中设置 `auth_type = "bearer"`，并把 token 写入 `[api].api_key` 或安装时的 `CORTEX_API_KEY`。

## 安全边界

- 本地模型减少 provider 暴露，但不是沙箱。
- Policy lint 和 risk gate 是审查控制，不是隔离机制。
- 在理解工具路径前，保持 `balanced` 或 `strict` 权限。
- 初始阶段保持插件禁用，之后只启用已审查的进程插件。
- 除非用途明确且受治理，不要把 secret 放进 prompt、retrieved file、demo workspace 或 durable memory。
- Retrieved file 和模型输出仍然是证据，不是 runtime instruction。
