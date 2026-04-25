#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v rg >/dev/null 2>&1; then
    echo "error: ripgrep is required for suppression scanning" >&2
    exit 1
fi

pattern='#!?\[(allow|expect)\(|#\[cfg_attr\([^\]]*(allow|expect)\('

if rg -n "${pattern}" --glob '*.rs' .; then
    echo "error: Rust warning suppression attributes are forbidden" >&2
    exit 1
fi

echo "ok: no Rust warning suppression attributes found"
