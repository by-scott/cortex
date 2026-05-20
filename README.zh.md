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

## 记忆与元认知

Cortex 的记忆系统是运行时层，不是对话记录的附录。一次 turn 可以把 actor 范围内的
相关证据召回到当前 runtime frame 中，用它保持连续性；这个 frame 在本次调用结束后
即被丢弃，避免旧上下文永久支配 prompt。新的记忆可以来自对话、用户明确要求和工具工作流，
并以结构化条目保存，带有 embedding、图关系、衰减和再巩固信息。这样 Cortex 能保留长程连续性，
同时仍把被召回的记忆视为证据：它可能过时、不完整，并且必须服从当前观察和运行时边界。

元认知是围绕记忆和工具循环的控制层。Cortex 会跟踪上下文压力、重复工具循环、疲劳、
耗时、置信度、被拒绝动作和 provider 健康等信号。这些信号可以触发压缩、策略提示、
restart boundary、权限暂停或操作者可见的告警。它的目标不是宣称模型具有“自我意识”，
而是让 harness 能发现 Agent 正在失去推进力、积累风险或携带过多陈旧上下文，并把 turn
导向更有纪律的恢复路径。

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
