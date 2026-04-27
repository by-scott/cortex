#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

run=false
check=false

usage() {
    cat <<'USAGE'
Usage: scripts/soak-fault-harness.sh [--check] [--run]

Runs or checks the bounded CI-compatible soak/fault harness for release review.

--check  Verify that all fault evidence surfaces exist.
--run    Run the bounded suites and print a report.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --check) check=true; shift ;;
        --run) run=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *)
            echo "error: unknown soak/fault argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

required_files=(
    "crates/cortex-runtime/src/tests/http_rpc.rs"
    "crates/cortex-runtime/src/tests/line_protocol.rs"
    "crates/cortex-runtime/src/tests/ws_rpc.rs"
    "crates/cortex-runtime/src/tests/channel_store.rs"
    "crates/cortex-runtime/tests/process_plugin.rs"
    "crates/cortex-kernel/tests/persistence_replay.rs"
    "crates/cortex-kernel/tests/config_loader.rs"
    "crates/cortex-kernel/tests/session_store_compat.rs"
    "crates/cortex-kernel/tests/task_audit_compat.rs"
)

for path in "${required_files[@]}"; do
    if [ ! -f "$path" ]; then
        echo "error: required soak/fault evidence file missing: $path" >&2
        exit 1
    fi
done

if "$check"; then
    echo "ok: bounded soak/fault harness prerequisites present"
    exit 0
fi

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

generated_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
git_rev="$(git rev-parse --short HEAD 2>/dev/null || printf 'unknown')"

cat <<REPORT
# Cortex Bounded Soak/Fault Harness

- Generated: ${generated_at}
- Git revision: ${git_rev}
- Scope: bounded CI-compatible fault evidence for v1.5.5

| Fault Class | Status | Command |
|-------------|--------|---------|
REPORT

run_status "provider schema/HTTP/RPC recovery" cargo test -p cortex-runtime http_rpc --all-features
run_status "channel reconnect and line protocol continuity" cargo test -p cortex-runtime line_protocol --all-features
run_status "websocket reconnect and ownership continuity" cargo test -p cortex-runtime ws_rpc --all-features
run_status "channel store migration and offset recovery" cargo test -p cortex-runtime channel_store --all-features
run_status "plugin crash, timeout, invalid output, and path faults" cargo test -p cortex-runtime --test process_plugin --all-features
run_status "SQLite, replay, migration, and side-effect recovery" cargo test -p cortex-kernel --test persistence_replay --all-features
run_status "config and corrupt legacy state recovery" cargo test -p cortex-kernel --test config_loader --all-features
run_status "session/task/audit compatibility recovery" sh -c 'cargo test -p cortex-kernel --test session_store_compat --all-features && cargo test -p cortex-kernel --test task_audit_compat --all-features'

cat <<'REPORT'

## Fault Classes Covered

- provider timeout/schema invalid: HTTP/RPC turn paths and model/provider routing surfaces.
- channel reconnect: socket, stdio, WebSocket, and channel store offset recovery.
- SQLite/replay: WAL-backed persistence, replay diffs, side-effect substitution, and migration fixtures.
- plugin crash: process plugin non-zero exit, timeout, invalid JSON, output limits, and path containment.
- disk/config faults: invalid legacy JSON/MsgPack/config files default safely or fail visibly.
- rate-limit/backpressure: foreground queue, cancellation, `/stop`, and hidden-session rejection paths.
- replay-after-upgrade: compatibility fixtures and projection-version checks.

Long 24h/72h/7d daemon soak remains a separate release attachment. If it is
absent, the release notes must say so.
REPORT
