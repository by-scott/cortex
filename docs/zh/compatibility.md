# 兼容性策略

Cortex 仍然是早期本地运行时。本文定义哪些 surface 现在应被视为兼容性边界，哪些只是 best-effort，哪些则在运行时继续加固期间仍有意保持不稳定。

目标很直接：在 Cortex 宣称更大生态之前，先把 operator trust 和扩展契约说清楚。

## 兼容性分层

### 已足够稳定、可被依赖

这些 surface 被视为 operator-facing 的兼容性边界，不应随意漂移：

- 持久化事件的 replay 语义
- `TurnState` 生命周期表面
- 安装期和运行期权限模式：`strict`、`balanced`、`open`
- `docs/plugins.md` 中记录的 process-plugin manifest surface
- trusted native ABI 的入口名：`cortex_plugin_init`
- 已发布 CLI / 文档中的 channel/operator 命令表面

这类变更需要：

- 明确的文档更新
- 回归测试覆盖
- 当 operator-facing 行为变化时，在 release note 中显式说明

### 带版本的契约表面

这些 surface 允许演进，但必须通过显式版本或契约边界来演进：

- trusted native ABI（`abi_version`）
- process-plugin manifest 字段与执行规则
- 写入事件的 replay execution version
- SDK DTO 与 media/tool-result 表面

这类变更需要：

- 版本提升或明确兼容性说明
- conformance tests
- 清晰失败的 migration 或 rejection 行为

### Best-Effort 表面

这些行为应保持连贯，但 Cortex 目前还不承诺超出当前 release line 的长期兼容：

- 内部 prompt 组装细节
- 元认知启发式与阈值
- skill utility 启发式
- status 展示格式
- 文档行为之下的 hot-reload 实现细节

这些内容可以随运行时质量改进而变化，但对外文档描述的行为仍应保持可读和 operator-safe。

## Event 与 Replay 兼容性

事件 Journal 是 source of truth。Cortex 把 replay 语义当成兼容性表面，而不只是调试便利功能。

当前策略：

- 可以新增事件
- 已持久化字段不应静默改变含义
- 只要仍在支持的 release line 内，replay 就应继续理解旧的 `execution_version`
- compaction boundary、side-effect substitution 和 replay digest 语义必须保持文档化并持续受测

如果 replay 语义发生 operator 能感知的变化，release note 应明确写出来。

## Process Plugin 兼容性

Process JSON 是默认外部插件边界。

当前策略：

- 文档记录过的 manifest 字段被视为公开契约表面
- 未文档化字段不算支持
- 路径规则、环境继承规则、timeout 行为和输出限制都属于兼容性相关行为
- invalid manifest 应明确失败，而不是静默降级

当 manifest surface 变化时，Cortex 应优先：

1. 增量式字段扩展
2. 对不支持的旧/新形式显式拒绝
3. 在同一次变更中同步 release note 和文档

## Trusted Native ABI 兼容性

Trusted native ABI 是带版本的扩展边界，不是沙箱。

当前策略：

- runtime 只加载 `cortex_plugin_init`
- ABI 兼容性通过 `abi_version` 控制
- 旧的 trait-object loading symbols 不属于受支持表面
- 对 invalid descriptor、invalid input、ABI mismatch 的失败回报应保持确定性

这里的兼容性意味着“要么明确版本化失败，要么明确成功加载”，而不是“所有旧二进制永远都能继续加载”。

## 文档兼容性

公开文档本身就是 operator contract 的一部分。Cortex 不应一边把文档当 normative surface，一边又允许它和 shipped runtime 漂移。

当前已有检查覆盖：

- README event 数量
- turn-state 表面
- permission-mode 指南
- plugin boundary 表述
- replay/compaction 表述
- 中英文 README 与 runtime surface 的关键硬表述

后续方向是把这类检查扩大成自动生成或 contract-tested operator docs，而不是继续依赖人工记忆。

## 发布预期

任何修改兼容性边界的 release，至少应说明：

- 改了什么
- 这是 additive、breaking，还是 rejection-only
- 是否需要 restart、reinstall 或 plugin rebuild
- 是否影响持久化数据或 replay 行为

如果某项变更还达不到这个标准，它就应继续留在“early/runtime hardening”范围内，而不应被包装成稳定平台行为。
