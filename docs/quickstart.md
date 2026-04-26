# Quickstart

## Build

```bash
cargo build --release --bin cortex
./target/release/cortex status
```

## Test

```bash
./scripts/gate.sh --docker
```

The gate requires zero Rust warning suppressions, `cargo fmt --all --check`,
strict clippy with pedantic and nursery lints, and full workspace tests.

## Package

```bash
./scripts/package-release.sh
```

The package script emits:

- `dist/cortex-v1.5.0-linux-amd64.tar.gz`
- `dist/cortex-v1.5.0-linux-amd64.tar.gz.sha256`

## Try The CLI

```bash
cortex version
cortex status
cortex release-plan
```

The 1.5 binary is currently an operator and contract surface. Live daemon
installation, channels, browser support, and tool execution are not restored
until their replacement implementations pass the same gate.
