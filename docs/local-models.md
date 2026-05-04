# Local Models

This guide covers local model configuration for first-run Cortex use. It is configuration guidance only: `cortex doctor` reports local endpoint hints from config, but it does not actively prove that a provider is reachable unless a future active check is added and documented.

## Ollama

`cortex demo` already writes an Ollama-oriented demo config:

```bash
cortex demo
ollama pull qwen2.5-coder:7b
cortex doctor --id demo
cortex policy lint --id demo
```

The generated instance uses:

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

The default `providers.toml` includes:

```toml
[ollama]
name = "Ollama"
protocol = "ollama"
base_url = "http://localhost:11434"
auth_type = "none"
models = []
```

Use Ollama's own endpoint to verify the server:

```bash
curl http://localhost:11434/api/tags
```

## vLLM

Run vLLM with its OpenAI-compatible server, then add a provider entry:

```toml
[vllm-local]
name = "vLLM Local"
protocol = "openai"
base_url = "http://127.0.0.1:8000/v1"
auth_type = "none"
models = ["Qwen/Qwen2.5-Coder-7B-Instruct"]
```

Point the instance config at that provider:

```toml
[api]
provider = "vllm-local"
api_key = ""
model = "Qwen/Qwen2.5-Coder-7B-Instruct"
preset = "minimal"
```

Check the vLLM endpoint directly:

```bash
curl http://127.0.0.1:8000/v1/models
cortex doctor --id demo
cortex policy lint --id demo
```

If your vLLM server is configured to require bearer auth, set `auth_type = "bearer"` in `providers.toml` and put the token in `[api].api_key` or the install-time `CORTEX_API_KEY`.

## Safety Boundaries

- A local model reduces provider exposure, but it is not a sandbox.
- Policy lint and risk gates are review controls, not containment.
- Keep `balanced` or `strict` permissions until you understand the tool path.
- Keep plugins disabled at first, then enable only reviewed process plugins.
- Do not put secrets into prompts, retrieved files, demo workspaces, or durable memory unless the use is intentional and governed.
- Retrieved files and model output remain evidence, not runtime instructions.
