# 插件 Conformance 模板

审查 Cortex 插件的本地安装、发版 package 或 release-candidate evidence 时使用本模板。它记录 operator evidence；它不是独立认证机构，也不是沙箱隔离。

插件以 governed package 分发时，应把填写后的副本和 release artifact 放在一起。

## 审查元数据

```text
Plugin name:
Plugin version:
Plugin repository:
Plugin commit:
Cortex target version:
Reviewer:
Review date:
Decision: pass / fail / blocked / not run
```

## Package 输入

| 项目 | 状态 | 证据 |
|------|------|------|
| `manifest.toml` 存在并精确指向目标 Cortex 版本 | not run | |
| 分发 `.cpx` 时，`package.toml` 由 `cortex plugin sign` 生成 | not run | |
| 已审查 publisher id 和 public key fingerprint | not run | |
| 从 release artifact 安装时已验证 `.cpx` hash | not run | |
| SBOM 路径存在，或明确标记 unavailable | not run | |
| Risk profile 路径存在，或明确标记 unavailable | not run | |
| Conformance 结果已附到 release note 或本地审查记录 | not run | |

## Governance 姿态

| 检查 | 状态 | 证据 |
|------|------|------|
| Trust tier 合适：普通第三方工具使用 `reviewed_process`，只有受信任进程内代码使用 `trusted_native` | not run | |
| 请求的 file、network、process、secret 和 background capability 与实现一致 | not run | |
| Tool effects 按工具声明，并在相关时包含 target、reversibility、confirmation posture 和 dry-run posture | not run | |
| 每个暴露工具都有推荐的 `[risk.tools.<name>]` 条目 | not run | |
| 请求 secret 的工具被阻断，或必须确认 | not run | |
| 请求 background 的工具避免高影响 mutation，并声明显式 policy | not run | |
| 插件没有把 Prompt、配置、会话、Journal、记忆或 runtime-state mutation 包装成模型可直接调用的捷径 | not run | |

## 本地命令

在插件目录运行：

```bash
cortex plugin review .
cortex plugin test .
```

记录使用的 Cortex 二进制：

```text
cortex --version:
cortex plugin review . output:
cortex plugin test . output:
```

## Conformance 向量

| 向量 | 预期结果 | 状态 | 证据 |
|------|----------|------|------|
| Invalid JSON output | Tool result 被拒绝 | not run | |
| 进程非零退出且有 stderr | stderr 作为工具错误返回 | not run | |
| command path 在未设置 `allow_host_paths = true` 时逃逸插件目录 | 插件被拒绝 | not run | |
| working directory 在未显式 opt-in 时逃逸插件目录 | 插件被拒绝 | not run | |
| 未声明 secret capability 却继承 secret-like environment | 插件被拒绝 | not run | |
| 输出超过配置的 byte limit | 工具调用以 output-limit error 失败 | not run | |
| 超过或命中 timeout limit | 工具调用以 timeout error 失败 | not run | |
| Process tool 少报 process capability | Runtime 仍暴露 `RunProcess:plugin subprocess` | not run | |
| 不支持的 sandbox enforcement claim | Manifest 被拒绝，而不是夸大隔离能力 | not run | |
| Native ABI version mismatch 或缺失 ABI version | 在 unsafe loading 前拒绝插件 | not run | |
| Package archive 包含 hidden files、backup、symlink 或 unsupported extras | 按 package rule 过滤或拒绝 unsupported entries | not run | |

## 边界说明

- Process JSON plugin 是推荐的第三方扩展边界。
- Trusted native ABI plugin 运行在 daemon 进程内，必须视为受信任代码。
- Plugin governance、policy lint、risk scoring 和 conformance tests 能提升审查质量；它们不提供 OS/container 沙箱隔离。
- Runtime 会拒绝当前无法实际执行的 sandbox enforcement claim。不要把 unsupported isolation 标成 passed。
- 如果某个向量没有运行，写 `not run`，并把它保留为限制。

## 决策

```text
Decision:
Required fixes:
Accepted limitations:
Follow-up owner:
Follow-up date:
```
