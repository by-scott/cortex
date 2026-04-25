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

require_text "README.md" "cortex-retrieval"
require_text "README.zh.md" "cortex-retrieval"
require_text "docs/retrieval.md" "cortex-turn::context::format_evidence_context"
require_text "docs/zh/retrieval.md" "cortex-turn::context::format_evidence_context"
require_text "docs/executive.md" "Retrieved evidence context"
require_text "docs/zh/executive.md" "Retrieved evidence"
require_text "docs/testing.md" "crates/cortex-runtime/src/tests/retrieval_context.rs"

echo "ok: docs surface drift checks passed"
