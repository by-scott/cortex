# Agent 维护流程

本文说明人类维护者、Codex、Claude Code 和 Cortex agent 的本地长期维护流程。它只是公开流程说明。不要提交私有的 `meta/` 目录。

## 本地 Meta 文件

维护者可以把跨会话上下文放在：

```text
meta/PROJECT_META.md
meta/status.md
```

规则：

- 修改前读取 `meta/PROJECT_META.md` 和 `meta/status.md`。
- 会话结束前更新 `meta/status.md`。
- 保持 `meta/` 被 git 忽略。
- 不要把 `status.md` 放在仓库根目录。
- 不要把未实现能力写成已实现。
- 不要把 policy/risk gate 描述成沙箱隔离。

如果需要本地模板，可从 [Status Template](../templates/status-template.md) 复制到本机 `meta/status.md`。

## 每次会话必须记录

每次维护会话应记录：

- 修改文件；
- 执行的命令和测试；
- 做出的决策；
- 风险和阻塞；
- 下一步；
- 是否运行了 full workspace tests、clippy、release gate，或只运行 focused checks。

涉及 runtime 或能力变化时，应写明相关安全边界：journal/replay、ownership、memory、RAG evidence separation、tool effects、plugin governance 和 protected runtime root。

## Git 边界

`meta/` 是本地运行上下文，不应 stage 或 commit。如果 `git status --ignored meta` 没显示 `meta/` 被忽略，应先明确地把 `/meta/` 加到本地 exclude 或 `.gitignore`，再继续维护。
