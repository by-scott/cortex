# 快速开始

## 安装发布二进制

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
cortex status
```

安装脚本会下载最新 GitHub release asset，校验 sha256，并默认把 `cortex`
安装到 `~/.local/bin`。

## 构建

```bash
cargo build --release --bin cortex
./target/release/cortex status
```

## 测试

```bash
./scripts/gate.sh --docker
```

gate 要求 0 Rust warning suppression、`cargo fmt --all --check`、严格 clippy
pedantic / nursery，以及完整 workspace tests。

## 打包

```bash
./scripts/package-release.sh
```

打包脚本输出：

- `dist/cortex-v1.5.0-linux-amd64.tar.gz`
- `dist/cortex-v1.5.0-linux-amd64.tar.gz.sha256`

## 试用 CLI

```bash
cortex version
cortex status
cortex release-plan
```

## 试用 Daemon

```bash
cortex daemon --data-dir /tmp/cortex-data --socket /tmp/cortex.sock
cortex register-tenant --socket /tmp/cortex.sock --tenant default --name Default
cortex bind-client --socket /tmp/cortex.sock --tenant default --actor local --client cli
cortex send --socket /tmp/cortex.sock --tenant default --actor local --client cli "hello"
cortex status --socket /tmp/cortex.sock
cortex stop --socket /tmp/cortex.sock
```

1.5 binary 目前是 daemon-first runtime 和 operator surface。Installer-managed
service setup、live channels、browser support、media tools、native plugin
loading 要等替代实现通过同一 gate 后再恢复。
