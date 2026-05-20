# Cortex使用指南

语言：[English](../usage.md) | 简体中文

本文是Cortex的完整操作手册，覆盖安装、首次初始化、日常CLI使用、实例、服务、
配置、权限、policy、插件、渠道、托管工具、dashboard、源码构建、skill治理和故障排查。

相关文档：[README](../../README.zh.md)、[插件开发指南](plugin-development.md)

## 运行模型

Cortex有两个用户可见入口，以及一个持久运行时边界：

- `cortex` CLI是操作者入口。它用于启动交互会话、发送单次prompt、管理systemd服务、安装插件、修改部分配置，并检查运行状态。
- Daemon是长期运行的runtime。它拥有RPC、socket、HTTP、渠道投递、插件、记忆、journal状态、prompt layers和内置dashboard。
- 实例是身份与状态的持久边界。每个实例都有自己的配置、数据、记忆、sessions、prompts、skills、channels和启用插件列表。

默认base directory是`~/.cortex`，默认实例ID是`default`。大多数日常操作只需要
`cortex`和`cortex "prompt"`；服务命令用于安装、重启、检查和修复daemon。

核心概念：

| 概念 | 含义 |
| --- | --- |
| Instance | Cortex base directory下的命名运行时身份。 |
| Session | 某个instance内的一段对话范围。 |
| Actor | 跨CLI、HTTP/RPC、socket、stdio和消息渠道共享的工作身份。 |
| Skill | 从system、instance或plugin skill目录加载的可复用`SKILL.md`流程，带运行时health和演化治理。 |
| Plugin | 经过声明和签名的能力包，可提供tools、skills、prompts或可信native code。 |
| Permission mode | 决定哪些工具副作用可以不经交互确认直接执行的运行时规则。 |

## 安装

发布安装脚本会根据当前平台下载GitHub Release archive，把`cortex`二进制安装到默认目录，
然后运行`cortex install`。

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
```

默认二进制安装位置：

- 用户安装：`~/.local/bin/cortex`
- 带`--system`的系统安装：`/usr/local/bin/cortex`

常用安装脚本环境变量：

```sh
CORTEX_VERSION=1.6.15
CORTEX_REPO=by-scott/cortex
CORTEX_INSTALL_DIR="$HOME/.local/bin"
CORTEX_INSTALL_ARGS="--id work --permission-level strict"
```

安装system-level service：

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --system --permission-level balanced
```

如果希望user service在用户退出登录后仍继续运行，请启用systemd linger：

```sh
sudo loginctl enable-linger "$USER"
```

## 首次Provider配置

`cortex install`会在daemon启动前读取首次配置环境变量。请在运行安装脚本或直接运行
`cortex install`前设置provider、model和key。

OpenAI-compatible示例：

```sh
export CORTEX_PROVIDER=openai
export CORTEX_MODEL=gpt-4.1
export CORTEX_API_KEY=sk-...
cortex install --permission-level balanced
```

自定义provider endpoint：

```sh
export CORTEX_PROVIDER=sglang
export CORTEX_BASE_URL=http://127.0.0.1:11990
export CORTEX_MODEL=qwen3.6-27b
export CORTEX_API_KEY=sk-local
cortex install --permission-level balanced
```

Embedding endpoint：

```sh
export CORTEX_EMBEDDING_PROVIDER=ollama
export CORTEX_EMBEDDING_BASE_URL=http://127.0.0.1:11434
export CORTEX_EMBEDDING_MODEL=nomic-embed-text
export CORTEX_EMBEDDING_API_KEY=sk-local
```

搜索与thinking控制：

```sh
export CORTEX_BRAVE_KEY=...
export CORTEX_SHOW_THINKING=false
```

常用首次安装环境变量：

| 变量 | 用途 |
| --- | --- |
| `CORTEX_PROVIDER` | LLM provider名称。 |
| `CORTEX_MODEL` | LLM model名称。 |
| `CORTEX_API_KEY` | LLM API key。 |
| `CORTEX_BASE_URL` | 自定义provider base URL。 |
| `CORTEX_LLM_PRESET` | Preset：`minimal`、`standard`、`cognitive`或`full`。 |
| `CORTEX_EMBEDDING_PROVIDER` | Embedding provider名称。 |
| `CORTEX_EMBEDDING_MODEL` | Embedding model名称。 |
| `CORTEX_EMBEDDING_BASE_URL` | Embedding provider base URL。 |
| `CORTEX_EMBEDDING_API_KEY` | Embedding provider API key。 |
| `CORTEX_SHOW_THINKING` | 在provider支持时启用thinking request/output。 |
| `CORTEX_BRAVE_KEY` | Brave Search API key。 |
| `CORTEX_PERMISSION_LEVEL` | 与`--permission-level`相同。 |

## 首次交互

启动CLI：

```sh
cortex
```

新实例会进入bootstrap模式。首次对话中，Cortex会询问实例名称、协作方式和需要跨会话
保留的偏好。可以把这一步看作是在设定这个Cortex实例以后如何与你协作；identity layer
形成后，后续turn会使用正常prompt layers。

第一次会话建议简短、明确：

```text
把这个实例命名为我的主工作站。
偏好简洁状态更新；破坏性操作前先确认；记住这个工作区用于Cortex开发。
```

然后让Cortex确认当前状态：

```text
总结你的实例身份、当前权限模式和可用工具。
```

单次prompt模式：

```sh
cortex "summarize the current repository status"
```

选择实例：

```sh
cortex --id work
cortex --id work "what are you currently configured to do?"
```

选择base directory：

```sh
cortex --home ~/.cortex-lab --id research
```

## CLI模式

主要模式：

```sh
cortex
cortex "question"
cortex --daemon
cortex --acp
cortex --mcp-server
```

普通CLI用于日常operator工作。`--daemon`是服务安装使用的runtime service模式。
`--acp`和`--mcp-server`为支持对应协议的客户端提供bridge。

`--acp`表示让其它支持ACP的客户端连接到Cortex。如果要让Cortex主动连接其它ACP agent，
使用运行时`acp`工具。

全局选项：

```sh
cortex --home <PATH>
cortex --id <ID>
cortex --new-process-plugin <NAME>
cortex --help
cortex --version
```

`--new-process-plugin`会在当前目录创建一个进程隔离插件脚手架。

## ACP Agent Client

Cortex可以通过`acp`工具主动连接外部ACP agent。当某个专门agent应该负责特定任务、
另一个workspace或一个受边界约束的子问题时使用这个能力。

`acp`工具支持`add`、`remove`、`list`、`status`、`connect`、`disconnect`和`prompt`。
`add`和`remove`会把client列表持久化到实例`[acp].clients`配置中，因此daemon重启后仍保留。
初始化握手也可以由工具指定：当某个agent要求协议变体时，可以设置`initialize_format`、
`protocol_version`、`client_name`和`client_version`。`initialize_format`支持
`standard`、`codex`或`hybrid`。正常ACP agent，例如`codex-acp`，使用`standard`；
Cortex只会在直连实验性的`codex exec-server`时使用Codex专用握手。

添加本地ACP agent：

```json
{
  "action": "add",
  "agent_id": "codex",
  "command": "codex-acp",
  "args": [],
  "cwd": "/work/repo"
}
```

如果不想全局安装npm包，也可以把`npx`作为命令：

```json
{
  "action": "add",
  "agent_id": "codex-npx",
  "command": "npx",
  "args": ["@zed-industries/codex-acp"],
  "cwd": "/work/repo"
}
```

添加通过SSH连接的ACP agent。Cortex会在本地启动`ssh runtime`，然后通过stdio与远端命令
使用ACP通信。`ssh_host`也支持`host:port`、`user@host:port`和`[ipv6]:port`：

```json
{
  "action": "add",
  "agent_id": "runtime-codex",
  "ssh_host": "runtime",
  "command": "/home/scott/.local/bin/codex-acp",
  "args": [],
  "cwd": "/home/scott/project/cortex",
  "initialize_format": "standard",
  "protocol_version": "1",
  "client_name": "cortex",
  "client_version": "1.6.15"
}
```

显式连接并创建session：

```json
{
  "action": "connect",
  "agent_id": "runtime-codex",
  "new_session": true
}
```

发送prompt。如果client尚未连接，Cortex会先连接：

```json
{
  "action": "prompt",
  "agent_id": "runtime-codex",
  "prompt": "Inspect the repository and summarize the current branch state."
}
```

使用`list`、`status`、`disconnect`和`remove`查看并管理运行时连接池。

## 日常工作流

感觉运行状态不对时，先看服务健康状态：

```sh
cortex status
cortex doctor
```

探索性工作、较长任务和多轮协作使用交互CLI：

```sh
cortex
```

请求本身很明确、适合从shell、编辑器任务或脚本触发时，使用单次prompt：

```sh
cortex "review the last commit and list behavioral risks"
```

需要隔离身份、记忆、配置和插件启用状态时，使用命名实例：

```sh
cortex --id work
cortex --id lab "what plugins are enabled here?"
```

变更运行时能力时，优先使用这个闭环：

```sh
cortex plugin install dev
cortex policy lint
cortex doctor
cortex restart
```

可信in-process native plugin code需要重启后才会加载。大部分配置、进程插件、skill和prompt
变更会热加载，但当运行时状态不确定时，`restart`是清晰的运维修复动作。

把skill演化当作常规运行时维护：

```text
/skill list
/skill proposals
/skill accept <proposal-id-or-prefix>
/skill reject <proposal-id-or-prefix>
```

生成的instance skill应该像代码一样审查：先检查proposal对应的`SKILL.md`，确认新workflow应该替代或改进旧skill时再接受；信号噪声大或场景太窄时拒绝。接受proposal只会更新生命周期状态，不会删除旧source。

## 实例与服务

安装或重装user service：

```sh
cortex install --permission-level balanced
cortex install --id work --permission-level strict
```

安装system service：

```sh
cortex install --system --permission-level balanced
```

管理服务状态：

```sh
cortex status
cortex start
cortex stop
cortex restart
cortex ps
```

移除服务：

```sh
cortex uninstall
cortex uninstall --id work
cortex uninstall --purge
```

`uninstall`会移除systemd service。带`--purge`时也会删除实例数据。

保留`config.toml`，清理运行数据：

```sh
cortex reset
cortex reset --id work --force
```

Factory reset：

```sh
cortex reset --factory --force
```

默认reset会保留配置，清理data、memory、sessions、prompts和skills。
Factory reset会删除整个实例目录并重新创建。

## 运行时目录结构

默认user layout：

```text
~/.cortex/
  providers.toml
  plugins/
  default/
    config.toml
    config.defaults.toml
    actors.toml
    mcp.toml
    data/
      cortex.sock
      cortex.db
      embedding_store.db
      memory_graph.db
      blobs/
    memory/
    sessions/
    prompts/
    skills/
    channels/
```

System instance使用`/var/lib/cortex`作为base directory。`--home <PATH>`选择另一个
base directory；`--id <ID>`选择该base下的实例。

Base-level `plugins/`目录保存已安装插件包。每个实例通过自己的`config.toml`决定启用哪些
已安装插件。

## 配置

显示配置摘要：

```sh
cortex config list
```

读取section：

```sh
cortex config get api
cortex config get providers
cortex config get embedding
cortex config get turn
cortex config get plugins
```

支持的section名称包括：

```text
api, context, memory, embedding, metacognition, turn, autonomous,
tools, acp, providers, daemon, web, skills, auth, risk, rate_limit,
health, evolution, ui, tls, plugins, mcp, llm_groups, memory_share
```

更新当前支持写入的key：

```sh
cortex config set turn.show_thinking false
cortex config set turn.strip_think_tags true
cortex config set embedding.api_key sk-local
```

可写key：

| Key | 效果 |
| --- | --- |
| `turn.show_thinking` | `true`启用provider thinking request/output。 |
| `turn.strip_think_tags` | `true`隐藏provider thinking output。 |
| `embedding.api_key` | 更新embedding provider API key。 |

配置会写入`config.toml`。用户daemon正在运行时会热加载；system instance的配置变更可能需要重启。

## 权限

权限模式决定哪些工具副作用可以不经交互确认直接执行：

| 模式 | 行为 |
| --- | --- |
| `strict` | 只自动批准Allow级别效果。 |
| `balanced` | 自动批准到Review级别效果。默认值。 |
| `open` | 自动批准所有非阻塞工具。 |

查看当前模式：

```sh
cortex permission
```

修改模式：

```sh
cortex permission strict
cortex permission balanced
cortex permission open
```

System instance：

```sh
cortex permission balanced --system
```

## Policy检查

Lint当前配置和启用插件：

```sh
cortex policy lint
```

模拟一次工具/effect决策：

```sh
cortex policy simulate bash --effect run_process:/tmp --actor user:local
cortex policy simulate web_fetch --effect network_request:https://example.com
```

Simulation选项：

```sh
cortex policy simulate <tool> --actor <actor> --effect <kind:target> --background
```

Effect kinds包括`read_file`、`read_secret`、`write_file`、`delete_file`、`run_process`、
`network_request`、`send_message`、`spend_money`、`deploy`、`modify_credential`、
`persist_memory`、`publish_content`、`schedule_task`、`generate_media`、
`introspect_runtime`和`delegate_work`。

## 插件

从短名称安装：

```sh
cortex plugin install dev
```

短名称会解析为GitHub仓库`github.com/by-scott/cortex-plugin-<name>`。

从指定仓库、release package或目录安装：

```sh
cortex plugin install by-scott/cortex-plugin-dev@1.6.10
cortex plugin install ./cortex-plugin-dev-v1.6.10-linux-amd64.cpx
cortex plugin install .
```

Packaged installs需要有效Ed25519 package signature。只有在审查新的verified publisher key
之后才使用`--yes`。

管理插件：

```sh
cortex plugin list
cortex plugin enable dev
cortex plugin disable dev
cortex plugin uninstall dev
cortex plugin uninstall dev --purge
```

审查、测试、签名和打包本地插件：

```sh
cortex plugin review .
cortex plugin test .
cortex plugin keygen ~/.config/cortex/plugin-signing/example.ed25519
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example.ed25519 --publisher example.dev
cortex plugin pack .
```

替换native插件后需要`cortex restart`，因为shared library会加载进daemon进程。
进程插件的可见性会随配置变更热加载，但重启是安全的运维修复手段。

## Skills

Skills是可复用的`SKILL.md`操作流程，可以来自system、instance或plugin skill目录。
通常通过插件安装skills。Instance skills位于实例`skills/`目录，daemon watcher会热加载。

Cortex也会跟踪skill health。每次执行都会更新utility score和状态：
`strong`、`healthy`、`needs_review`、`quarantined`或`deprecated`。Strong和healthy
skills会在自动summary中排得更靠前；needs_review会降低排序；quarantined和deprecated会
保留用于检查，但不会被Agent自动激活。

Health状态：

| 状态 | 运行时行为 |
| --- | --- |
| `strong` | 经常有效；在summary和自动激活中优先级更高。 |
| `healthy` | 正常可用skill。 |
| `needs_review` | 仍可用，但优先级较低，需要检查。 |
| `quarantined` | 保留用于审查，但不会自动激活。 |
| `deprecated` | 作为历史或fallback保留，但不会自动激活。 |

反复出现且有效的工具调用模式可以生成新的instance级skill候选。Cortex只会在目标
`SKILL.md`不存在时写入新文件，然后创建evolution proposal，而不是覆盖旧source。
Proposal可以表示new pattern、improvement、alternative或对弱skill的candidate replacement。
接受proposal会把candidate标记为healthy，并把target标记为deprecated；拒绝proposal不会改动
两个source。Proposal决策会持久化并写入journal。

Proposal关系：

| 关系 | 含义 |
| --- | --- |
| `new_pattern` | Cortex发现了没有接近已有skill的重复workflow。 |
| `improves` | Candidate看起来能改进已有skill。 |
| `alternative_to` | Candidate用不同方式覆盖相同工具模式。 |
| `candidate_replacement` | Candidate可能替代弱或quarantined skill。 |

在交互CLI中查看skills和proposal：

```text
/skill list
/skill proposals
/skill accept <proposal-id-or-prefix>
/skill reject <proposal-id-or-prefix>
```

查看skills配置：

```sh
cortex config get skills
```

开发插件skills请阅读[插件开发指南](plugin-development.md)。

## 渠道

当渠道auth material存在时，渠道会在daemon内运行。

查看渠道配置提示：

```sh
cortex channel telegram
cortex channel whatsapp
cortex channel qq
cortex channel qclaw
```

常用渠道环境变量：

```sh
export CORTEX_TELEGRAM_TOKEN=...
export CORTEX_WHATSAPP_TOKEN=...
export CORTEX_QQ_APP_ID=...
export CORTEX_QQ_APP_SECRET=...
export CORTEX_QQ_MARKDOWN=true
export CORTEX_QCLAW_TOKEN=...
```

QClaw是微信iLink adapter。它可以使用已有token配置，也可以走二维码登录流程：

```sh
cortex channel qclaw login
cortex restart
```

如果iLink部署需要自定义endpoint或路由tag，可以使用高级登录参数：

```sh
cortex channel qclaw login --base-url https://ilinkai.weixin.qq.com --route-tag <tag>
```

配对和用户策略：

```sh
cortex channel pair telegram
cortex channel approve telegram 123456 --subscribe
cortex channel subscribe telegram 123456
cortex channel unsubscribe telegram 123456
cortex channel revoke telegram 123456
cortex channel policy telegram whitelist
cortex channel allow telegram 123456
cortex channel deny telegram 999999
cortex channel unallow telegram 123456
cortex channel undeny telegram 999999
```

渠道policy modes：

- `pairing`：用户需要pair并被批准。
- `whitelist`：只有allow list中的用户可以交互。
- `open`：按该transport的auth模型开放。

## Actor身份

Actor alias和transport binding可以把transport-specific身份映射到canonical actor，
让HTTP、RPC、websocket、socket、stdio和聊天渠道共享一致的session与memory owner。

```sh
cortex actor alias list
cortex actor alias set telegram:123 user:scott
cortex actor alias unset telegram:123
cortex actor transport list
cortex actor transport set all user:scott
cortex actor transport unset telegram
```

`all`会绑定`http`、`rpc`、`ws`、`sock`和`stdio`。

## Node.js与浏览器集成

Node.js tooling用于需要Node或pnpm的MCP servers：

```sh
cortex node status
cortex node setup
```

浏览器集成会配置Chrome DevTools MCP server：

```sh
cortex browser status
cortex browser enable
cortex browser disable
```

设置后运行`cortex doctor`，确认daemon可见必要本地工具和路径。

## Dashboard与RPC

`cortex status`会显示服务状态、PID、socket path、data directory、HTTP address、
当前LLM provider/model/preset、权限模式、context和token usage。

```sh
cortex status
```

Dashboard由daemon通过内置assets提供，不依赖远程CDN JavaScript或CSS。

## 从源码构建

Docker是标准构建环境：

```sh
./scripts/build.sh
```

当前构建门禁运行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTFLAGS="-D warnings" cargo build --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

调试单个步骤：

```sh
docker compose build dev
docker compose run --rm dev cargo check --workspace --all-features --locked
```

## 故障排查

运行readiness checks：

```sh
cortex doctor
cortex doctor --json
```

`doctor`会检查OS和systemd可用性、实例路径、service/socket状态、配置、provider key姿态、
权限模式、启用插件、渠道auth、policy lint结果、受保护runtime root path和本地模型endpoint提示。

常见修复入口：

```sh
cortex status
cortex restart
cortex config list
cortex config get api
cortex config get providers
cortex policy lint
cortex plugin list
cortex reset --force
```

常见现象：

| 现象 | 优先检查 |
| --- | --- |
| CLI无法连接 | `cortex status`、socket path、service state。 |
| 模型调用失败 | `cortex config get api`、provider key、base URL、model name。 |
| Embedding失败 | `cortex config get embedding`、endpoint、API key。 |
| 插件工具缺失 | `cortex plugin list`、`cortex restart`、plugin review output。 |
| 渠道用户无法交互 | `cortex channel pair <platform>`、channel policy、allow/deny lists。 |
| 浏览器工具不可用 | `cortex browser status`、`cortex node status`、`cortex doctor`。 |
| Thinking文本泄露 | `cortex config get turn`，然后设置`turn.strip_think_tags true`。 |

从`cortex status`和`cortex doctor`开始排查；它们会告诉你当前实际使用的实例、路径、
service unit、socket和provider config。
