#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v rg >/dev/null 2>&1; then
    echo "error: ripgrep is required for suppression scanning" >&2
    exit 1
fi

attribute_pattern='#!?\[\s*(allow|expect)\s*\(|#!?\[\s*cfg_attr\s*\([^\]]*(allow|expect)\s*\('

if rg -n "${attribute_pattern}" --glob '*.rs' --glob '!target/**' .; then
    echo "error: Rust warning suppression attributes are forbidden" >&2
    exit 1
fi

flag_pattern='(^|[[:space:]"'"'"'])(-A[[:space:]]+(warnings|[a-z_]+|clippy::[a-z_]+)|--cap-lints([=[:space:]]+)allow|RUSTFLAGS=.*-A[[:space:]])'

if rg -n "${flag_pattern}" \
    --glob '*.sh' \
    --glob '*.toml' \
    --glob '*.yml' \
    --glob '*.yaml' \
    --glob 'Dockerfile' \
    --glob 'Makefile' \
    --glob '!scripts/check-suppressions.sh' \
    --glob '!target/**' \
    .; then
    echo "error: compiler warning suppression flags are forbidden" >&2
    exit 1
fi

echo "ok: no Rust warning suppression attributes or compiler suppression flags found"
