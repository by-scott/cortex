#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v rg >/dev/null 2>&1; then
    echo "error: ripgrep is required for secret scanning" >&2
    exit 1
fi

patterns=(
    "$(printf '/home/%s/' 'scott')"
    'ciof[[:alnum:]_-]{20,}'
    'sk-[A-Za-z0-9_-]{20,}'
    '-----BEGIN (RSA|OPENSSH|PRIVATE) KEY-----'
)

for pattern in "${patterns[@]}"; do
    if output="$(rg -n \
        --glob '!target/**' \
        --glob '!target-release/**' \
        --glob '!Cargo.lock' \
        -- "${pattern}" \
        . 2>&1)"; then
        printf '%s\n' "$output"
        echo "error: public tree matched forbidden secret/path pattern: ${pattern}" >&2
        exit 1
    else
        status=$?
        if [ "$status" -gt 1 ]; then
            printf '%s\n' "$output" >&2
            echo "error: secret scan failed for pattern: ${pattern}" >&2
            exit 1
        fi
    fi
done

echo "ok: no forbidden secret or personal-path patterns found"
