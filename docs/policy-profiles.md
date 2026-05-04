# Policy Profiles

`profiles/` contains sample `config.toml` fragments for common local-first postures. They are not loaded automatically by a `cortex profile` command yet. Copy the relevant sections into `~/.cortex/<instance>/config.toml`, then verify the instance:

```bash
cortex doctor --json
cortex policy lint
```

Policy profiles adjust risk scoring, confirmation, allowlists, denylists, disabled tools, plugin enablement, memory extraction, and provider defaults. They are review controls, not sandbox containment.

## Available Profiles

| File | Use |
|------|-----|
| `profiles/personal-local.toml` | Trusted single-user local work with balanced review posture and plugins disabled. |
| `profiles/coding-agent.toml` | Local repository work with web/channel tools disabled, plugins disabled, and write/process actions kept behind confirmation. |
| `profiles/local-vllm.toml` | Local OpenAI-compatible vLLM setup paired with conservative coding-tool policy. |
| `profiles/strict-safe.toml` | High-caution read/review posture with mutating, process, web, MCP, and memory-save tools denied. |
| `profiles/mcp-gateway.toml` | Starting point for reviewed MCP/plugin gateway use; replace example tool names with concrete registered tools. |

## Applying A Profile

1. Install or create the target instance.
2. Open `~/.cortex/<instance>/config.toml`.
3. Copy only the profile sections you intend to use.
4. Keep comments near copied policy sections so future operators know the intent.
5. Run `cortex doctor --json` and `cortex policy lint`.
6. For high-impact tools, run `cortex policy simulate <tool> --effect <effect> --actor <actor>` before use.

For local vLLM, also add the provider registry entry shown in [Local Models](local-models.md). `profiles/local-vllm.toml` references `local-vllm`, but the provider registry still lives in `~/.cortex/providers.toml`.

## Safety Boundaries

- Keep `open` permissions out of shared, plugin-heavy, or side-effect-heavy workflows.
- Keep unreviewed plugins disabled. For reviewed plugins, add concrete `[risk.tools.<name>]` entries for every exposed tool.
- Avoid wildcard allowlists for tools that can write files, send messages, deploy, publish, rotate credentials, or spend money.
- Keep automatic durable memory extraction disabled for profiles that ingest network, MCP, plugin, or other hostile evidence.
- Treat native plugins as trusted in-process code. These profiles do not make native code sandboxed.
- Use OS/container isolation separately if you need containment.
