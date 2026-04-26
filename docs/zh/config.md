# 配置

Cortex 1.5 有一个小型 daemon bootstrap 格式，用于初始化 tenants 和 clients。
运行时状态由类型化 contract 表达，并通过 `cortex-kernel` 的 SQLite store
持久化。

## Daemon Bootstrap

```json
{
  "tenants": [
    {"id": "default", "name": "Default"}
  ],
  "clients": [
    {
      "tenant_id": "default",
      "actor_id": "local",
      "client_id": "cli",
      "max_chars": 4096
    }
  ]
}
```

启动 daemon：

```bash
cortex daemon --data-dir /var/lib/cortex --socket /run/cortex.sock --config bootstrap.json
```

## 构建与 Gate 变量

| 变量 | 用途 |
| --- | --- |
| `CORTEX_GATE_IMAGE` | `scripts/gate.sh --docker` 使用的 Docker 镜像名 |
| `CORTEX_GATE_CARGO_VOLUME` | Cargo cache 的 Docker volume |
| `CORTEX_GATE_IN_DOCKER` | gate 容器内部标记 |
| `CORTEX_PACKAGE_PLATFORM` | 发布包平台后缀，默认 `linux-amd64` |
| `CORTEX_DIST_DIR` | 发布输出目录，默认 `dist` |
| `SOURCE_DATE_EPOCH` | 可复现打包时间戳 |

## 持久化状态

当前 SQLite store 覆盖：

- tenants、clients、active sessions；
- actor-visible sessions 和最小权限 1.4 session import；
- fast captures 和 semantic memories；
- permission requests 和 resolutions；
- per-recipient delivery outbox records；
- side-effect intent/result records；
- owner-filtered token usage records。

所有持久化对象都必须携带 ownership。跨 tenant 或跨 actor 访问必须在读取
或修改私有状态之前被拒绝。
