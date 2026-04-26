# 快速开始

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

1.5 binary 目前是 operator 和 contract surface。Live daemon install、channels、
browser support、tool execution 要等替代实现通过同一 gate 后再恢复。
