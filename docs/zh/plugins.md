# 插件

Cortex 1.5 暴露 SDK contract，不暴露 live plugin loader。

## SDK Contract

- `PluginManifest::validate` 拒绝空 name、空 version 和 ABI mismatch。
- `PluginManifest::validate_request` 拒绝需要未声明 capability 的 tool
  request。
- `PluginContext::authorize` 拒绝 host 未授予的 capability。
- host path 默认拒绝，除非 host 显式启用。
- `ToolResponse::validate_output` 强制 output limit。

## ABI

`cortex-sdk` 声明 `ABI_VERSION`。plugin manifest 必须匹配该值，host 才能
把插件视为 conforming。

## 当前边界

当前发布不加载 native shared library，也不启动 process plugin。执行路径必须
带 conformance tests 重建后，才能重新写成运维功能。
