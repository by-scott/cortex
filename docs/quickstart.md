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

## Try The Daemon

```bash
cortex daemon --data-dir /tmp/cortex-data --socket /tmp/cortex.sock
cortex register-tenant --socket /tmp/cortex.sock --tenant default --name Default
cortex bind-client --socket /tmp/cortex.sock --tenant default --actor local --client cli
cortex send --socket /tmp/cortex.sock --tenant default --actor local --client cli "hello"
cortex status --socket /tmp/cortex.sock
cortex stop --socket /tmp/cortex.sock
```

The 1.5 binary is currently a daemon-first runtime and operator surface.
Installer-managed service setup, live channels, browser support, media tools,
and native plugin loading are restored only after their replacement
implementations pass the same gate.
