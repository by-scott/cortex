# 插件

Cortex 1.5 暴露 SDK contract 和 runtime tool execution boundary，但还不暴露
native shared-library loading 或 process plugin spawning。

## SDK Contract

- `PluginManifest::validate` 拒绝空 name、空 version 和 ABI mismatch。
- `PluginManifest::validate_request` 拒绝需要未声明 capability 的 tool
  request。
- `PluginContext::authorize` 拒绝 host 未授予的 capability。
- host path 默认拒绝，除非 host 显式启用。
- `ToolResponse::validate_output` 强制 output limit。

## Runtime Execution

`CortexRuntime::execute_tool` 会验证 manifest、验证 tool request、构造
host-granted `PluginContext`、在创建 side effect 前拒绝未授权 host path、
记录 `SideEffectIntended`、通过 runtime `ToolExecutor` 执行 tool、校验 output
size，并记录 `SideEffectRecorded`。

执行失败也必须是 durable record，不是无声 host error。成功记录携带 output
digest。Tool side effect 默认对调用 client 私有，除非未来边界显式扩大 scope。

## ABI

`cortex-sdk` 声明 `ABI_VERSION`。plugin manifest 必须匹配该值，host 才能
把插件视为 conforming。

## 当前边界

当前发布不加载 native shared library，也不启动 process plugin。loader 路径
必须带 conformance tests 重建后，才能重新写成运维功能。
