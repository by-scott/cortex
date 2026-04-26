# Usage

## Commands

```bash
cortex version
cortex status
cortex release-plan
cortex help
```

Unknown commands exit with status code `2` and print help.

## Runtime Contracts

Use the crate APIs for integration tests and embedding:

- `cortex-types` for ownership, events, retrieval, delivery, deployment, and
  usage DTOs.
- `cortex-kernel` for file journal and SQLite state store.
- `cortex-retrieval` for ownership-filtered RAG.
- `cortex-turn` for planning and provider calls.
- `cortex-runtime` for tenant/client/session binding, authenticated ingress,
  and delivery routing.
- `cortex-sdk` for plugin manifest and tool request conformance.

The CLI is intentionally small while the runtime is being rebuilt.
