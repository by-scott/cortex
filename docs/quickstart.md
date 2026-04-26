# Quickstart

## Install Published Binary

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
cortex status
```

The installer downloads the latest GitHub release asset, verifies its sha256,
and installs the `cortex` binary into `~/.local/bin` by default.

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
setup, channels, browser support, and tool execution are not restored until
their replacement implementations pass the same gate.
