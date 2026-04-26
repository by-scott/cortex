# 使用

## 命令

```bash
cortex version
cortex status
cortex release-plan
cortex help
```

未知命令以状态码 `2` 退出并打印 help。

## Runtime Contracts

集成测试和嵌入使用 crate API：

- `cortex-types`：ownership、events、retrieval、delivery、deployment、usage DTOs。
- `cortex-kernel`：file journal 和 SQLite state store。
- `cortex-retrieval`：ownership-filtered RAG。
- `cortex-turn`：planning 和 provider calls。
- `cortex-runtime`：tenant/client/session binding、authenticated ingress、
  delivery routing。
- `cortex-sdk`：plugin manifest 和 tool request conformance。

CLI 在 runtime 重建期间保持很小。
