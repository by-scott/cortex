#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

require_text() {
    local file="$1"
    local text="$2"
    if ! grep -Fq "$text" "$file"; then
        echo "error: ${file} is missing required text: ${text}" >&2
        exit 1
    fi
}

require_file() {
    local file="$1"
    if [ ! -f "$file" ]; then
        echo "error: missing required file: ${file}" >&2
        exit 1
    fi
}

require_file "README.md"
require_file "README.zh.md"
require_file "docs/retrieval.md"
require_file "docs/zh/retrieval.md"
require_file "docs/executive.md"
require_file "docs/zh/executive.md"
require_file "docs/testing.md"

require_text "README.md" "Cortex 1.5.0 is the daemon-first production-core rebuild line"
require_text "README.md" "production-grade multi-user"
require_text "README.md" "crates/cortex-runtime/tests/multi_user.rs"
require_text "README.zh.md" "1.5.0 还不是 1.4 所有用户可见功能的完整替代"
require_text "README.zh.md" "crates/cortex-runtime/tests/multi_user.rs"
require_text "docs/retrieval.md" "Evidence cannot become durable memory implicitly"
require_text "docs/zh/retrieval.md" "Evidence 不能隐式变成 durable memory"
require_text "docs/executive.md" "ExpectedControlValue"
require_text "docs/zh/executive.md" "ExpectedControlValue"
require_text "docs/testing.md" "crates/cortex-types/tests/mechanisms.rs"
require_text "docs/testing.md" "cross-tenant and cross-actor access is denied"

echo "ok: docs surface drift checks passed"
