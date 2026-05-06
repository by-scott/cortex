#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

run=false
check=false
mode="docker"

usage() {
    cat <<'USAGE'
Usage: scripts/release-behavior-report.sh [--check] [--run] [--docker|--host]

Builds the release behavior evidence report for the current checkout.

--check  Verify that the report surface is present without running tests.
--run    Run the targeted behavior suites and include pass/fail statuses.
--docker Use the repository docker-compose dev service. This is the default and
         is the release authority.
--host   Run directly on the host. This is only a developer shortcut.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --check) check=true; shift ;;
        --run) run=true; shift ;;
        --docker) mode="docker"; shift ;;
        --host) mode="host"; shift ;;
        -h|--help) usage; exit 0 ;;
        *)
            echo "error: unknown report argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ "$mode" = "docker" ] && [ "${CORTEX_RELEASE_REPORT_IN_DOCKER:-}" != "1" ] && "$run"; then
    if ! command -v docker >/dev/null 2>&1; then
        echo "error: docker is required for the authoritative release behavior report" >&2
        exit 1
    fi
    if [ ! -f docker-compose.yml ]; then
        echo "error: docker-compose.yml is required for the authoritative release behavior report" >&2
        exit 1
    fi
    docker compose build dev >/dev/null
    exec docker compose run --rm \
        -e CORTEX_RELEASE_REPORT_IN_DOCKER=1 \
        dev \
        ./scripts/release-behavior-report.sh --host --run
fi

required_files=(
    "docs/release-evidence/template.md"
    "docs/plugin-conformance-template.md"
    "docs/prompt-injection-corpus.md"
    "scenarios/prompt-injection/corpus.json"
    "docs/actor-leakage-corpus.md"
    "scenarios/actor-leakage/corpus.json"
    "docs/replay-migration-corpus.md"
    "scenarios/replay-migration/corpus.json"
    "docs/release-audit-1.6.7.md"
    "docs/testing.md"
    "scripts/daemon-soak.sh"
    "crates/cortex-turn/tests/memory_tools.rs"
    "crates/cortex-retrieval/tests/rag_pipeline.rs"
    "crates/cortex-turn/tests/safety_contracts.rs"
    "crates/cortex-runtime/src/tests/http_operator.rs"
    "crates/cortex-runtime/src/tests/http_rpc.rs"
    "crates/cortex-kernel/tests/persistence_replay.rs"
)

for path in "${required_files[@]}"; do
    if [ ! -f "$path" ]; then
        echo "error: required report evidence file missing: $path" >&2
        exit 1
    fi
done

if "$check"; then
    echo "ok: release behavior report prerequisites present"
    exit 0
fi

git_rev="$(git rev-parse --short HEAD 2>/dev/null || printf 'unknown')"
generated_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

run_status() {
    local label="$1"
    shift
    if ! "$run"; then
        printf '| %s | not run | `%s` |\n' "$label" "$*"
        return 0
    fi
    if "$@"; then
        printf '| %s | pass | `%s` |\n' "$label" "$*"
    else
        printf '| %s | fail | `%s` |\n' "$label" "$*"
        return 1
    fi
}

cat <<REPORT
# Cortex Release Behavior Report

- Generated: ${generated_at}
- Git revision: ${git_rev}
- Release target: v1.6.7
- Gate authority: \`./scripts/gate.sh --docker\`

This report is the release behavior evidence surface. It is not a replacement
for the strict Docker gate; it records which behavior suites support the release
claims around memory, retrieval, tools, safety, recovery, and soak posture.

## Targeted Behavior Suites

| Area | Status | Command |
|------|--------|---------|
REPORT

run_status "memory ownership and memory tools" cargo test -p cortex-turn --test memory_tools --all-features
run_status "retrieval/RAG metrics and support verification" cargo test -p cortex-retrieval --all-features
run_status "tool risk, permissions, and guardrail safety corpus" cargo test -p cortex-turn --test safety_contracts --all-features
run_status "operator metrics and timeline observability" cargo test -p cortex-runtime http_operator --all-features
run_status "long-task interruption and recovery RPC paths" cargo test -p cortex-runtime http_rpc --all-features
run_status "journal replay and side-effect recovery" cargo test -p cortex-kernel --test persistence_replay --all-features

cat <<'REPORT'

## Required Release Attachments

- Full strict gate output from `./scripts/gate.sh --docker`.
- Final `docs/release-audit-1.6.7.md` state.
- This behavior report generated with `--run`.
- Prompt-injection corpus review from `scenarios/prompt-injection/corpus.json`.
- Actor leakage corpus review from `scenarios/actor-leakage/corpus.json`.
- Replay migration corpus review from `scenarios/replay-migration/corpus.json`.
- Bounded soak/fault report from `./scripts/soak-fault-harness.sh --run`.
- Long daemon soak report from `./scripts/daemon-soak.sh --run --duration 24h --interval 60s` before claiming 24h soak evidence.

## Metric Coverage

- Memory: actor ownership, memory save/search behavior, and replayed evidence.
- Actor isolation: session, memory, task, goal, retrieval, channel, transport, and operator-route scoping.
- Retrieval: recall/MRR helpers, citation keys, support verification, negative evidence, taint, and actor scoping.
- Tools: risk floor, permission behavior, declared effects, preview/verify/commit surface, and plugin process controls.
- Safety: prompt-injection/exfiltration corpus across web, file, retrieval, plugin, channel, and tool-shaped inputs.
- Recovery: journal replay, side-effect substitution, RPC cancellation, live `/stop`, hidden-session rejection, and current replay fixtures.
- Replay migration: current replay fixtures, projection versions, replay diffs, side-effect substitution, and historical fixture limitations.
- long-task recovery: cancellation, interruption, replay determinism, and hidden-session rejection evidence.
- Soak: release candidates must attach a bounded or long-running soak/fault report; absent soak evidence must remain a public release limitation.
REPORT
