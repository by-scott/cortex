#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

mode="docker"
require_clean=false
image="${CORTEX_GATE_IMAGE:-cortex-gate:1.95.0}"

usage() {
    cat <<'USAGE'
Usage: scripts/gate.sh [--docker|--host] [--require-clean]

Runs the Cortex strict gate:
  - no Rust warning suppression attributes
  - cargo fmt with no diff
  - docs/package/secret drift checks
  - cargo clippy --workspace --all-targets --all-features with -D warnings,
    clippy::pedantic, and clippy::nursery
  - cargo test --workspace --all-features

--docker is the release authority. --host is only a developer shortcut.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --docker) mode="docker"; shift ;;
        --host) mode="host"; shift ;;
        --require-clean) require_clean=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *)
            echo "error: unknown gate argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

check_clean() {
    if [ -n "$(git status --short)" ]; then
        git status --short >&2
        echo "error: release gate requires a clean checkout" >&2
        exit 1
    fi
    git diff --exit-code >/dev/null
    git diff --cached --exit-code >/dev/null
}

run_host_gate() {
    if "$require_clean"; then
        check_clean
    fi

    ./scripts/check-suppressions.sh
    cargo fmt --all --check
    ./scripts/check-docs-drift.sh
    ./scripts/check-package-surface.sh
    ./scripts/check-secrets.sh
    cargo clippy --workspace --all-targets --all-features -- \
        -D warnings \
        -W clippy::pedantic \
        -W clippy::nursery
    cargo test --workspace --all-features
}

if [ "$mode" = "docker" ]; then
    if [ "${CORTEX_GATE_IN_DOCKER:-}" = "1" ]; then
        run_host_gate
        exit 0
    fi
    docker_args=(./scripts/gate.sh --host)
    if "$require_clean"; then
        docker_args+=(--require-clean)
    fi
    docker build --target dev -t "$image" .
    docker run --rm \
        -e CORTEX_GATE_IN_DOCKER=1 \
        -v "$PWD":/workspace \
        -w /workspace \
        "$image" \
        "${docker_args[@]}"
else
    run_host_gate
fi
