#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

workspace_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
sdk_version="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/cortex-sdk/Cargo.toml | head -1)"

if [ -z "$workspace_version" ]; then
    echo "error: workspace version is missing" >&2
    exit 1
fi

workspace_release_line="${workspace_version%.*}"
sdk_release_line="${sdk_version%.*}"

if [ "$sdk_release_line" != "$workspace_release_line" ]; then
    echo "error: cortex-sdk version ${sdk_version:-missing} does not match workspace release line ${workspace_release_line}.x" >&2
    exit 1
fi

if ! cargo metadata --format-version 1 --no-deps >/dev/null; then
    echo "error: cargo metadata failed" >&2
    exit 1
fi

if awk '
    /^\[dependencies\]/ { in_dependencies = 1; next }
    /^\[/ { in_dependencies = 0 }
    in_dependencies && /^[[:space:]]*cortex-/ { found = 1 }
    END { exit found ? 0 : 1 }
' crates/cortex-sdk/Cargo.toml; then
    echo "error: cortex-sdk must not depend on Cortex internal crates" >&2
    exit 1
fi

if ! grep -Fq 'cortex-v${VERSION}-${PLATFORM}.tar.gz' scripts/cortex.sh; then
    echo "error: installer asset naming no longer matches release packaging" >&2
    exit 1
fi

if [ ! -f Dockerfile ]; then
    echo "error: Dockerfile is required for release gate reproducibility" >&2
    exit 1
fi

if [ ! -f docker-compose.yml ]; then
    echo "error: docker-compose.yml is required for the repository Docker gate" >&2
    exit 1
fi

if ! grep -Fq 'dev:' docker-compose.yml || ! grep -Fq 'target: dev' docker-compose.yml; then
    echo "error: docker-compose.yml must define the dev gate service" >&2
    exit 1
fi

if ! grep -Fxq 'FROM rust:latest AS dev' Dockerfile; then
    echo "error: Dockerfile must use rust:latest as the repository gate image" >&2
    exit 1
fi

if ! grep -Fq 'rustup component add rustfmt clippy' Dockerfile; then
    echo "error: Dockerfile must install rustfmt and clippy for the release gate" >&2
    exit 1
fi

echo "ok: package surface checks passed for v${workspace_version} / cortex-sdk v${sdk_version}"
