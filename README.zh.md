<div align="center">

# Cortex

**面向持久、可治理 AI Agent 的认知运行时 harness。**

[English](README.md) · 简体中文

[![Release](https://img.shields.io/badge/release-1.6.10-blue)](https://github.com/by-scott/cortex/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95.0-orange)](Dockerfile)
[![Build](https://img.shields.io/badge/build-Docker-informational)](scripts/build.sh)
[![SDK](https://img.shields.io/badge/SDK-1.6.10-lightgrey)](https://docs.rs/cortex-sdk/latest/cortex_sdk/)

</div>

Cortex 与 Claude Code、Codex、OpenClaw 以及其他现代 coding runtime 属于同一类
agent harness：它们把 LLM 从对话模型接到文件、工具、记忆、策略和反馈循环上，
让模型不只是回答，而是能在真实工作空间里行动。

Cortex 的判断更进一步：长程 Agent 不能只依赖更长的 prompt 和更多工具。面向真实多会话工作的
harness 需要一套运行时模型，把注意力、记忆、权限、渠道、插件和副作用变成明确的
运维对象。Cortex 把这套模型实现为操作者可治理的运行时：Agent 应该能跨会话保持连续性，
能协调工具，能谨慎形成记忆，能解释发生过什么，也能让人类治理有后果的行动。

它的设计受认知科学和生产级运行时工程共同约束：全局工作空间、工作记忆、互补学习
系统、元认知、层级控制、事件溯源、持久执行和显式信任边界。这些不是修辞装饰，
而是用来塑造运行时边界的工程原则。

## 为什么需要 Cortex

成熟 harness 已经证明了核心交互形态：让模型检查工作区、调用工具、根据反馈迭代，
并与操作者协作。Cortex 从这个基线出发，聚焦第一轮惊艳演示之后真正困难的问题：
连续性、治理、记忆质量、渠道身份、插件信任和运维可见性。

Cortex 把 Agent 当作运行时问题来处理：

- 前台 turn 是有限注意力通道，不是无限追加的文本流。
- 记忆经过捕获、物化、稳定化，而不是松散追加笔记。
- 工具副作用经过策略、风险和权限门禁。
- 运行状态写入 journal，便于审计、理解和恢复。
- 插件与渠道是显式边界，能力必须声明。

## 记忆

Cortex 把记忆当作主动的运行时基质，而不是对话归档。每一次 turn 都会从当前对话、
actor 身份、策略、工具状态和被召回的证据中组装一个 request-local working frame。
这个 frame 是 Agent 临时的全局工作空间：记忆可以进入其中维持连续性，但它不是指令；
当它与当前观察、用户明确要求或运行时 schema 冲突时，必须让位。

长期记忆有自己的生命周期。新的候选记忆可以来自用户明确要求、工具证据、对话结果和
post-turn extraction。它们会作为结构化条目保存，包含 type、kind、owner actor、
source trust、evidence events、claim 字段、strength、时间戳、矛盾关系、supersession
关系、有效期、使用结果和反馈归因。记忆只有在访问模式、证据、用户确认和实际帮助程度
都支持时，才会从 `Captured` 进入 `Materialized`，再进入 `Stabilized`。弱记忆或陈旧记忆
会衰减到 `Deprecated`；被重新召回的稳定记忆会打开 reconsolidation window，让新证据
修订旧信念，而不是在旧信念旁边无限堆叠。

召回不是单一路径。Cortex 会综合 lexical relevance、embedding、recency、可靠性状态、
访问频率、actor 范围和图邻近度。memory graph 保存 dependency、preference、causality、
ownership、replacement、temporal order 等类型化关系；召回可以沿近邻节点扩展，并给多跳
上下文打分，而不是把完整历史塞回 prompt。这更接近“受控检索进入工作记忆”，不是不断变长的
scratchpad。

## 元认知

元认知是监督这套基质的控制循环。Cortex 会观察上下文压力、工作记忆容量与衰减、重复工具循环、
耗时、疲劳、frame anchoring、置信度、provider 与 embedding 健康、记忆碎片化、召回退化和
工具效用。这些信号会写入 journal，并可以触发压缩边界、策略提示、探索提示、skill activation、
权限暂停、恢复建议或记忆巩固。它不是在宣称模型拥有“自我意识”，而是让 harness 有足够的
自我观察能力，能发现 Agent 正在失去推进力、过度信任陈旧上下文，或把不确定性转化成副作用。

## 自演化

Cortex 支持受约束的自演化：运行时可以根据经验更新该更新的部分，但不会把系统的任意修改权
交给模型。post-turn analysis 会观察一组加权信号，例如用户纠正、明确偏好、新工作领域、
首次会话初始化证据、工具密集型 turn 和长输入。当这些信号足以说明需要适应时，Cortex 可以
通过基于证据的 self-update pass 更新 prompt layers。

这条路径有明确边界。最终回复草稿不会被当作 prompt 内容，更新必须通过 layer boundary 校验，
bootstrap 与增量演化使用不同规则，runtime policy 不属于 prompt self-update 的修改范围。
记忆演化也遵循同样纪律：记忆通过可审计事件进行拆分、巩固、稳定化、废弃和图关系重组，
而不是以不可见方式漂移 prompt。结果是一种操作者能观察、能回滚、能治理的适应机制。

## 能力

- 交互式 CLI、单次 prompt 管道模式、daemon 模式、ACP bridge 和 MCP server 模式。
- systemd user 或 system 服务部署，支持多个命名实例。
- LLM 与 embedding provider 配置，包括自定义 provider endpoint。
- strict、balanced、open 三种工具确认权限模式。
- 针对工具副作用的 policy lint 与 simulation。
- 插件安装、审查、一致性测试、签名、打包和运行时启用。
- 支持的消息渠道配对与策略管理。
- 面向 MCP 工作流的 Node.js 与浏览器集成管理。
- daemon 内置 dashboard 静态资源，不依赖远程 CDN。

## Dev 插件

[Cortex Dev Plugin](https://github.com/by-scott/cortex-plugin-dev) 是 Cortex 推荐的开发能力包。
它提供可信 native 的 tree-sitter 代码智能，覆盖 Rust、Python、TypeScript 和 TSX，
同时随包提供探索、实现、审查、调试、重构、测试、发布、故障处理、安全审查和规范提交等
workflow skills。当 Cortex 需要深入软件仓库完成工程任务时，应从已签名 release 安装这个插件。

## 使用

快速开始：

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
cortex doctor
cortex
```

带 provider 的首次安装：

```sh
export CORTEX_PROVIDER=openai
export CORTEX_MODEL=gpt-4.1
export CORTEX_API_KEY=sk-...

curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
```

daemon 安装后，用 `cortex` 进入交互式 CLI，并在对话中完成首次初始化。
用 `cortex "问题"` 可以执行单次 prompt。

## 文档

- [使用指南](docs/zh_CN/usage.md)
- [插件开发指南](docs/zh_CN/plugin-development.md)
- [SDK 指南](docs/zh_CN/sdk.md)

## 架构

Cortex 按清晰职责拆分为多个 Rust crate：

- `crates/cortex-app`：CLI、部署命令、服务管理和运维工作流。
- `crates/cortex-runtime`：daemon、HTTP/RPC、渠道、插件、dashboard 服务和运行时编排。
- `crates/cortex-turn`：turn 编排、LLM 调用、工具、skills、风险和记忆工作流。
- `crates/cortex-kernel`：配置、持久化、journal、policy、prompt 和存储原语。
- `crates/cortex-types`：共享 wire、config、event、memory、plugin 和 policy 契约。
- `crates/cortex-sdk`：面向可信 native 插件的公开 Rust SDK。
- `static`：内置 dashboard 静态资源。

## 安全模型

Cortex 默认假设模型输出、工具输出、插件数据、渠道消息、网络内容、记忆内容和
dashboard API 响应都是不可信的，除非它们通过显式边界。安全敏感行为围绕
fail-closed 权限检查、插件能力声明、policy simulation、可审计事件和内置
dashboard 资源构建。

可信 native 插件在进程内运行，发布前必须经过审查、测试、签名和打包。进程隔离
插件使用子进程 JSON 工具，并在 manifest 中声明命令、超时、环境变量和文件系统规则。

## 开发

Docker 是标准开发环境：

```sh
./scripts/build.sh
```

构建门禁会运行格式检查、严格 Clippy、workspace build、文档警告检查和仓库纪律扫描。
Cargo 命令使用 locked dependencies，Docker base image 固定 Rust 工具链。

## 许可证

Cortex 使用 [MIT License](LICENSE)。
