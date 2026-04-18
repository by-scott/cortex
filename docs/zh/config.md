# 配置参考

## 文件布局

```
~/.cortex/
  providers.toml              # 全局提供商注册表（热重载）
  plugins/                    # 全局插件存储
  <instance>/
    config.toml               # 实例配置（热重载）
    mcp.toml                  # MCP 服务器配置
    cortex.sock               # Unix 域套接字（位于 data/）
    prompts/
      soul.md                 # 存在导向 -- 通过经验成长
      identity.md             # 自我模型和架构
      behavioral.md           # 工具使用和技能指导
      user.md                 # 协作者档案
      .initialized            # 引导启动完成标记
      system/                 # 系统提示模板
        memory-extract.md, memory-consolidate.md, bootstrap.md,
        bootstrap-init.md, hint-exploration.md, hint-doom-loop.md,
        hint-fatigue.md, hint-frame-anchoring.md, context-compress.md,
        context-summarize.md, entity-extract.md, self-update.md,
        causal-analyze.md, batch-analysis.md, summarize-system.md,
        agent-readonly.md, agent-full.md, agent-teammate.md
    skills/
      system/                 # 内置技能（deliberate、diagnose、review、orient、plan）
      <custom>/SKILL.md       # 用户自定义技能
    data/
      cortex.db               # 事件日志（SQLite WAL）
      embedding_store.db      # 嵌入向量
      memory_graph.db         # 实体关系图
      cron_queue.json         # 计划任务
      model_info.json         # 缓存的提供商模型目录
      vision_caps.json        # 缓存的视觉能力探测
      defaults.toml           # 生成的默认值参考
      node/                   # Node.js 环境（由 `cortex node setup` 创建）
    memory/                   # 持久化记忆文件（.md 带 YAML frontmatter）
    sessions/                 # 会话历史
    channels/                 # 通道认证（telegram/、whatsapp/）
```

## 环境变量

仅在 `cortex install` 期间读取，用于生成 config.toml。

| 变量 | 用途 | 默认值 |
|------|------|--------|
| CORTEX_API_KEY | 提供商 API 密钥 | （必填） |
| CORTEX_PROVIDER | 提供商名称 | anthropic |
| CORTEX_MODEL | 模型标识符 | （提供商默认值） |
| CORTEX_BASE_URL | 自定义端点 URL | （提供商默认值） |
| CORTEX_LLM_PRESET | 子端点预设 | full |
| CORTEX_EMBEDDING_PROVIDER | 嵌入提供商 | ollama |
| CORTEX_EMBEDDING_MODEL | 嵌入模型 | nomic-embed-text |
| CORTEX_BRAVE_KEY | Brave Search API 密钥 | （空） |
| CORTEX_TELEGRAM_TOKEN | Telegram 机器人令牌 | （空） |
| CORTEX_WHATSAPP_TOKEN | WhatsApp 访问令牌 | （空） |

## config.toml

规范段落顺序：daemon、api、embedding、web、plugins、llm_groups、memory、turn、metacognition、autonomous、context、skills、auth、tls、rate_limit、tools、health、evolution、ui、memory_share、media。

---

### [daemon]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| addr | string | "127.0.0.1:0" | HTTP 监听地址。端口 0 自动分配随机端口；实际端口在首次绑定时持久化。 |
| maintenance_interval_secs | integer | 1800 | 心跳维护周期间隔（30 分钟）。 |
| model_info_ttl_hours | integer | 168 | 提供商模型目录缓存 TTL（7 天）。 |

---

### [api]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| provider | string | "anthropic" | 提供商名称（必须匹配 providers.toml 条目）。 |
| api_key | string | "" | 提供商 API 密钥。 |
| model | string | "" | 模型标识符。空值使用提供商的第一个列出模型。 |
| max_tokens | integer | 0 | 每次 LLM 调用的最大输出 token 数。0 使用系统回退值 300,000。 |
| preset | string | "full" | 子端点激活预设：minimal、standard、cognitive、full。 |

#### [api.endpoints]

每个端点的启用/禁用开关。预设设置默认值；显式值覆盖。

| 键 | 类型 | 描述 |
|----|------|------|
| memory_extract | bool | 从对话轮次中提取记忆 |
| entity_extract | bool | 提取实体关系 |
| compress | bool | 在压力阈值时压缩上下文 |
| summary | bool | 生成对话摘要 |
| self_update | bool | 从证据中演进提示层 |
| causal_analyze | bool | 分析因果模式 |
| autonomous | bool | 空闲时启用自主认知 |

预设：

| 预设 | 启用的端点 |
|------|-----------|
| minimal | 无 |
| standard | memory_extract、compress、entity_extract |
| cognitive | standard + self_update、causal_analyze、autonomous |
| full | 全部 7 个 |

#### [api.endpoint_groups]

将子端点映射到 `[llm_groups]` 中定义的命名 LLM 组：

```toml
[api.endpoint_groups]
memory_extract = "light"
entity_extract = "light"
compress = "light"
summary = "light"
self_update = "medium"
causal_analyze = "medium"
```

#### [api.vision]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| provider | string | "" | 视觉提供商名称 |
| model | string | "" | 视觉模型标识符 |

为空时使用自动发现：提供商的 vision_model，然后探测主模型。

---

### [embedding]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| provider | string | "ollama" | 嵌入提供商名称 |
| api_key | string | "" | 嵌入 API 密钥 |
| model | string | "nomic-embed-text" | 嵌入模型标识符 |
| dimensions | integer | 0 | 向量维度。0 使用原生维度。 |
| candidates | array | [] | 自动切换评估的候选模型 |
| min_samples | integer | 10 | 评估前的最小样本数 |
| auto_switch | bool | false | 启用自动模型切换 |
| switch_threshold_samples | integer | 50 | 切换前需要的样本数 |
| switch_precision_delta | float | 0.1 | 切换所需的精度提升 |

---

### [web]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| search_backend | string | "brave" | 网络搜索后端 |
| brave_api_key | string | "" | Brave Search API 密钥 |
| brave_max_results | integer | 10 | 每次搜索的默认最大结果数 |
| brave_max_results_limit | integer | 20 | 结果数硬上限 |
| fetch_max_chars | integer | 100000 | 页面抓取的默认最大字符数 |
| fetch_max_chars_limit | integer | 500000 | 抓取字符数硬上限 |

---

### [plugins]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| dir | string | "plugins" | 插件目录路径 |
| enabled | array | [] | 已启用的插件名称列表 |

插件使用 `MultiToolPlugin` FFI 接口，允许单个共享库一次注册多个工具。默认不启用任何插件；按需安装和启用。详见 [docs/plugins.md](plugins.md)。

---

### [llm_groups]

命名 LLM 配置，用于将子端点路由到不同的模型和提供商。安装时自动生成三个层级。

```toml
[llm_groups.heavy]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key = ""   # 空值从 [api] 继承
max_tokens = 0

[llm_groups.medium]
provider = "zai"
model = "glm-4.7"

[llm_groups.light]
provider = "zai"
model = "glm-4.5-air"
```

解析顺序：端点字段 > 组字段 > [api] 字段 > 提供商默认值。

---

### [memory]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| max_recall | integer | 10 | 每次查询的最大记忆召回数 |
| decay_rate | float | 0.05 | 记忆相关性衰减率 |
| auto_extract | bool | true | 自动从对话轮次中提取记忆 |
| extract_min_turns | integer | 5 | 触发提取前的最小轮次数 |
| consolidate_interval_hours | integer | 24 | 记忆整合周期间隔（小时） |

---

### [turn]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| max_tool_iterations | integer | 1024 | 每轮最大工具调用次数 |
| tool_timeout_secs | integer | 300 | 单个工具执行超时 |
| strip_think_tags | bool | true | 从 LLM 输出中剥离 `<think>` 标签 |

#### [turn.trace]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| level | string | "off" | 全局默认追踪级别 (off/minimal/basic/summary/full/debug) |
| tool | string | "minimal" | Tool 类别覆盖（默认 minimal） |
| phase | string | — | Phase 类别覆盖（默认使用全局） |
| llm | string | — | LLM 类别覆盖（默认使用全局） |
| meta | string | — | Meta 类别覆盖（默认使用全局） |
| memory | string | — | Memory 类别覆盖（默认使用全局） |
| context | string | — | Context 类别覆盖（默认使用全局） |
可用类别：phase、llm、tool、meta、memory、context。

---

### [metacognition]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| doom_loop_threshold | integer | 3 | 重复失败达此次数后触发干预 |
| duration_limit_secs | integer | 86400 | 轮次时长软警告阈值（仅记录警报，不中断） |
| fatigue_threshold | float | 0.8 | 触发干预的疲劳分数 |
| frame_anchoring_threshold | float | 0.5 | 触发重构的框架锚定分数 |

#### [metacognition.frame_audit]

| 键 | 类型 | 默认值 |
|----|------|--------|
| goal_stagnation_threshold | integer | 5 |
| monotony_threshold | float | 0.7 |
| correction_threshold | integer | 3 |
| failure_streak_threshold | integer | 3 |
| low_confidence_threshold | float | 0.3 |

权重（总和必须为 1.0）：

| 键 | 默认值 |
|----|--------|
| goal_stagnation | 0.25 |
| tool_monotony | 0.25 |
| correction | 0.20 |
| low_confidence | 0.15 |
| failure_streak | 0.15 |

#### [metacognition.rpe]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| low_utility_threshold | float | 0.5 | 奖赏预测误差效用下限 |
| drift_ratio_threshold | float | 10.0 | 触发重构的漂移比率 |

#### [metacognition.health_recovery]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| dimension_threshold | float | 0.7 | 触发恢复的单维度分数 |

#### [metacognition.denial]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| consecutive_threshold | integer | 3 | 连续拒绝达此次数后升级 |
| session_threshold | integer | 10 | 会话总拒绝次数达此后升级 |

---

### [autonomous]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| enabled | bool | true | 启用自主后台认知 |
| heartbeat_interval_secs | integer | 10 | 心跳检查间隔 |

#### [autonomous.thresholds]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| consolidate_count | integer | 5 | 整合前的待处理记忆数 |
| deprecate_check | bool | true | 检查过期记忆 |
| embed_pending | bool | true | 嵌入未嵌入的记忆 |
| skill_evolve_calls | integer | 100 | 技能演进检查前的工具调用次数 |
| reflection_idle_secs | integer | 3600 | 反思前的空闲秒数 |
| self_update_corrections | integer | 3 | 自更新前的纠正次数 |

#### [autonomous.limits]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| max_llm_calls_per_hour | integer | 10 | 每小时 LLM 调用预算 |
| max_concurrent | integer | 1 | 最大并发自主任务数 |
| cooldown_after_llm_secs | integer | 300 | LLM 调用后的冷却时间（5 分钟） |

---

### [context]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| max_tokens | integer | 200000 | 上下文窗口预算 |
| pressure_thresholds | array | [0.60, 0.75, 0.85, 0.95] | 四个压力级别 |

压力级别：

1. **正常**（< 0.60）-- 仅监控
2. **警告**（0.60）-- 压缩旧轮次
3. **压缩**（0.75）-- 激进压缩
4. **紧急**（0.85）-- 紧急处理，丢弃低优先级内容

---

### [skills]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| extra_dirs | array | [] | 额外的技能目录 |
| max_active_summaries | integer | 30 | 上下文中的最大技能摘要数 |
| default_timeout_secs | integer | 600 | 默认技能执行超时 |
| inject_summaries | bool | true | 将技能摘要注入系统提示 |

---

### [auth]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| enabled | bool | false | 启用 JWT 认证 |
| secret | string | "" | HMAC-SHA256 密钥 |
| token_expiry_hours | integer | 24 | 令牌过期时长 |

免认证端点：/api/health、/api/metrics/*、/api/auth/*、静态页面。

---

### [tls]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| enabled | bool | false | 启用 TLS |
| cert_path | string | "" | TLS 证书路径 |
| key_path | string | "" | TLS 私钥路径 |

---

### [rate_limit]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| per_session_rpm | integer | 10 | 每个会话每分钟请求数 |
| global_rpm | integer | 60 | 全局每分钟请求数 |

超出限制返回 HTTP 429，附带 Retry-After 头。免限制：/api/health、/api/metrics/*。

---

### [tools]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| disabled | array | [] | 已禁用的工具名称列表 |

热重载。禁用的工具对 LLM 不可见。

---

### [health]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| check_interval_turns | integer | 10 | 健康检查间隔轮次数 |
| degraded_threshold | float | 0.3 | 维度退化判定分数 |
| weights | array | [0.25, 0.25, 0.25, 0.25] | 维度权重 |

四个维度：记忆碎片化、上下文压力、召回退化、疲劳。

---

### [evolution]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| source_modify_enabled | bool | false | 允许修改提示源文件 |

信号权重：

| 信号 | 权重 |
|------|------|
| correction_weight | 1.0 |
| preference_weight | 0.8 |
| new_domain_weight | 0.6 |
| first_session_weight | 0.5 |
| tool_intensive_weight | 0.4 |
| long_input_weight | 0.3 |

加权分数 >= 0.5 时触发演进。

---

### [ui]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| prompt_symbol | string | "cortex> " | REPL 提示符号 |
| locale | string | "auto" | UI 语言区域 |

---

### [memory_share]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| mode | string | "disabled" | 共享模式：disabled、readonly、readwrite |
| instance_id | string | "" | 共享目标实例 ID |

---

### [media]

| 键 | 类型 | 默认值 | 描述 |
|----|------|--------|------|
| stt | string | "local" | 语音转文字提供商：local、openai、zai |
| tts | string | "edge" | 文字转语音提供商：edge、openai、zai |
| image_gen | string | "" | 图像生成：zai、openai、""（禁用） |
| video_gen | string | "" | 视频生成：zai、""（禁用） |
| image_understand | string | "" | 图像理解：""、zai、openai |
| video_understand | string | "" | 视频理解：zai、gemini、"" |
| api_key | string | "" | 共享媒体 API 密钥（空值继承 [api].api_key） |
| api_url | string | "" | 共享 URL 覆盖 |

每个能力的覆盖：

| 键 | 默认值 | 描述 |
|----|--------|------|
| stt_api_key | "" | STT 专用 API 密钥 |
| stt_api_url | "" | STT 专用端点 URL |
| whisper_model | "whisper" | Whisper 模型名称 |
| tts_api_key | "" | TTS 专用 API 密钥 |
| tts_api_url | "" | TTS 专用端点 URL |
| tts_voice | "zh-CN-XiaoxiaoNeural" | TTS 语音名称 |
| image_gen_api_key | "" | 图像生成 API 密钥 |
| image_gen_api_url | "" | 图像生成端点 URL |
| image_gen_model | "" | 图像生成模型 |
| image_understand_api_key | "" | 图像理解 API 密钥 |
| image_understand_api_url | "" | 图像理解端点 URL |
| image_understand_model | "" | 图像理解模型 |
| video_gen_api_key | "" | 视频生成 API 密钥 |
| video_gen_api_url | "" | 视频生成端点 URL |
| video_gen_model | "cogvideox-3" | 视频生成模型 |
| video_understand_api_key | "" | 视频理解 API 密钥 |
| video_understand_api_url | "" | 视频理解端点 URL |
| video_understand_model | "glm-4v-plus" | 视频理解模型 |

解析顺序：capability_key > media.api_key > [api].api_key

---

## mcp.toml

与 config.toml 同级的独立文件。定义外部 MCP 服务器连接。

stdio 传输：

```toml
[[servers]]
name = "fs"
transport = "stdio"
command = "npx"
args = ["-y", "@mcp/server-fs"]
env = { NODE_ENV = "production" }
```

SSE 传输：

```toml
[[servers]]
name = "remote"
transport = "sse"
url = "https://api.example.com/mcp"
headers = { Authorization = "Bearer token" }
```

工具命名约定：`mcp_{server_name}_{tool_name}`

---

## providers.toml

全局提供商注册表。热重载。

11 个内置提供商：

| 名称 | 协议 | Base URL | 默认模型 | 视觉模型 | 认证 |
|------|------|----------|----------|----------|------|
| anthropic | anthropic | https://api.anthropic.com | claude-sonnet-4-20250514 | -- | x-api-key |
| openai | openai | https://api.openai.com | gpt-4o | gpt-4o | bearer |
| zai | anthropic | https://api.z.ai/api/anthropic | glm-5.1 | GLM-4.6V | x-api-key |
| zai-openai | openai | https://api.z.ai/api/coding/paas/v4 | glm-5.1 | GLM-4.6V | bearer |
| zai-cn | anthropic | https://open.bigmodel.cn/api/anthropic | glm-4-plus | GLM-4.6V | x-api-key |
| zai-cn-openai | openai | https://open.bigmodel.cn/api/paas/v4 | glm-4-plus | GLM-4.6V | bearer |
| kimi | openai | https://api.moonshot.cn/v1 | moonshot-v1-auto | -- | bearer |
| kimi-cn | openai | https://api.moonshot.cn/v1 | moonshot-v1-auto | -- | bearer |
| minimax | openai | https://api.minimax.chat/v1 | abab6.5s-chat | -- | bearer |
| openrouter | openai | https://openrouter.ai/api | -- | -- | bearer |
| ollama | ollama | http://localhost:11434 | -- | -- | none |

自定义提供商格式：

```toml
[my-provider]
name = "My Provider"
protocol = "openai"   # anthropic, openai, ollama
base_url = "https://api.example.com/v1"
auth_type = "bearer"  # bearer, x-api-key, none
models = ["model-a"]
vision_model = "model-v"
```

---

## 热重载

| 文件 | 行为 |
|------|------|
| config.toml | 下次轮次时生效 |
| providers.toml | 立即生效 |
| prompts/*.md | 立即生效 |
| skills/*/SKILL.md | 重启时生效（新增）或立即生效（修改） |

使用 `notify` crate，500ms 防抖。
