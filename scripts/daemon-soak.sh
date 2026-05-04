#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

run=false
check=false
mode="docker"
duration="24h"
interval="60s"
instance_id="${CORTEX_DAEMON_SOAK_ID:-soak}"
home_dir="${CORTEX_DAEMON_SOAK_HOME:-}"
report_dir="${CORTEX_DAEMON_SOAK_REPORT_DIR:-}"

usage() {
    cat <<'USAGE'
Usage: scripts/daemon-soak.sh [--check] [--run] [options] [--docker|--host]

Runs or checks the long daemon soak runner. The default run duration is 24h.

--check              Verify that daemon soak prerequisites exist.
--run                Build Cortex, start a direct daemon process, and sample it.
--duration DURATION  Run duration such as 30s, 10m, 24h, 72h, or 7d.
--interval DURATION  Sample interval. Defaults to 60s.
--id NAME            Instance id to use. Defaults to soak.
--home PATH          Cortex base home. Defaults to a temporary directory.
--report-dir PATH    Artifact directory. Defaults to dist/daemon-soak-<timestamp>.
--docker             Use the repository docker-compose dev service. This is the
                     default and the release authority.
--host               Run directly on the host. This is only a developer shortcut.

Samples `cortex doctor --json` and `cortex policy lint` throughout the run.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --check) check=true; shift ;;
        --run) run=true; shift ;;
        --duration) duration="$2"; shift 2 ;;
        --interval) interval="$2"; shift 2 ;;
        --id) instance_id="$2"; shift 2 ;;
        --home) home_dir="$2"; shift 2 ;;
        --report-dir) report_dir="$2"; shift 2 ;;
        --docker) mode="docker"; shift ;;
        --host) mode="host"; shift ;;
        -h|--help) usage; exit 0 ;;
        *)
            echo "error: unknown daemon soak argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

parse_duration_secs() {
    local value="$1"
    local number unit

    if [[ ! "$value" =~ ^([0-9]+)([smhd]?)$ ]]; then
        echo "error: invalid duration '$value' (use seconds, or s/m/h/d suffix)" >&2
        exit 1
    fi

    number="${BASH_REMATCH[1]}"
    unit="${BASH_REMATCH[2]:-s}"
    case "$unit" in
        s|"") echo "$number" ;;
        m) echo $((number * 60)) ;;
        h) echo $((number * 3600)) ;;
        d) echo $((number * 86400)) ;;
        *)
            echo "error: invalid duration unit '$unit'" >&2
            exit 1
            ;;
    esac
}

duration_secs="$(parse_duration_secs "$duration")"
interval_secs="$(parse_duration_secs "$interval")"
if [ "$duration_secs" -le 0 ] || [ "$interval_secs" -le 0 ]; then
    echo "error: duration and interval must be greater than zero" >&2
    exit 1
fi

if [ "$mode" = "docker" ] && [ "${CORTEX_DAEMON_SOAK_IN_DOCKER:-}" != "1" ] && "$run"; then
    if ! command -v docker >/dev/null 2>&1; then
        echo "error: docker is required for the authoritative daemon soak runner" >&2
        exit 1
    fi
    if [ ! -f docker-compose.yml ]; then
        echo "error: docker-compose.yml is required for the authoritative daemon soak runner" >&2
        exit 1
    fi
    docker compose build dev >/dev/null
    args=(--host --run --duration "$duration" --interval "$interval" --id "$instance_id")
    if [ -n "$home_dir" ]; then
        args+=(--home "$home_dir")
    fi
    if [ -n "$report_dir" ]; then
        args+=(--report-dir "$report_dir")
    fi
    exec docker compose run --rm \
        -e CORTEX_DAEMON_SOAK_IN_DOCKER=1 \
        dev \
        ./scripts/daemon-soak.sh "${args[@]}"
fi

required_files=(
    "Dockerfile"
    "docker-compose.yml"
    "crates/cortex-app/src/cli.rs"
    "crates/cortex-app/src/deploy.rs"
    "docs/testing.md"
    "scripts/soak-fault-harness.sh"
)

for path in "${required_files[@]}"; do
    if [ ! -f "$path" ]; then
        echo "error: required daemon soak surface missing: $path" >&2
        exit 1
    fi
done

if "$check"; then
    echo "ok: daemon soak runner prerequisites present"
    exit 0
fi

if ! "$run"; then
    usage
    exit 0
fi

classification="bounded smoke (not 24h evidence)"
if [ "$duration_secs" -ge 604800 ]; then
    classification="7d daemon soak evidence candidate"
elif [ "$duration_secs" -ge 259200 ]; then
    classification="72h daemon soak evidence candidate"
elif [ "$duration_secs" -ge 86400 ]; then
    classification="24h daemon soak evidence candidate"
fi

generated_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
stamp="$(date -u '+%Y%m%dT%H%M%SZ')"
git_rev="$(git rev-parse --short HEAD 2>/dev/null || printf 'unknown')"
if [ -z "$report_dir" ]; then
    report_dir="dist/daemon-soak-${stamp}"
fi
mkdir -p "$report_dir"

cleanup_home=false
if [ -z "$home_dir" ]; then
    home_dir="$(mktemp -d "${TMPDIR:-/tmp}/cortex-daemon-soak-home.XXXXXX")"
    cleanup_home=true
fi
mkdir -p "$home_dir"

daemon_pid=""
cleanup() {
    if [ -n "${daemon_pid:-}" ] && kill -0 "$daemon_pid" >/dev/null 2>&1; then
        kill "$daemon_pid" >/dev/null 2>&1 || true
        wait "$daemon_pid" >/dev/null 2>&1 || true
    fi
    if "$cleanup_home"; then
        rm -rf "$home_dir"
    fi
}
trap cleanup EXIT INT TERM

echo "Building Cortex daemon binary..."
cargo build -p cortex-app --bin cortex >/dev/null
bin="target/debug/cortex"

"$bin" demo --home "$home_dir" --id "$instance_id" --force \
    >"${report_dir}/demo.out" 2>"${report_dir}/demo.err"

"$bin" --home "$home_dir" --id "$instance_id" --daemon \
    >"${report_dir}/daemon.log" 2>&1 &
daemon_pid="$!"

socket_path="${home_dir}/${instance_id}/data/cortex.sock"
ready=false
for _ in $(seq 1 30); do
    if ! kill -0 "$daemon_pid" >/dev/null 2>&1; then
        echo "error: daemon exited before becoming ready" >&2
        tail -80 "${report_dir}/daemon.log" >&2 || true
        exit 1
    fi
    if [ -S "$socket_path" ]; then
        if "$bin" doctor --home "$home_dir" --id "$instance_id" --json \
            >"${report_dir}/doctor-ready.json" 2>"${report_dir}/doctor-ready.err" \
            && grep -Fq "daemon reachable" "${report_dir}/doctor-ready.json"; then
            ready=true
            break
        fi
    fi
    sleep 1
done

if ! "$ready"; then
    echo "error: daemon did not become ready within 30s" >&2
    tail -80 "${report_dir}/daemon.log" >&2 || true
    exit 1
fi

samples="${report_dir}/samples.tsv"
printf 'timestamp\tpid_alive\tdoctor_rc\tdaemon_reachable\tpolicy_lint_rc\n' >"$samples"

start_epoch="$(date +%s)"
deadline=$((start_epoch + duration_secs))
iterations=0
failures=0

while [ "$(date +%s)" -lt "$deadline" ]; do
    iterations=$((iterations + 1))
    timestamp="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    pid_alive="yes"
    doctor_rc=0
    policy_rc=0
    daemon_reachable="no"

    if ! kill -0 "$daemon_pid" >/dev/null 2>&1; then
        pid_alive="no"
        failures=$((failures + 1))
    fi

    if "$bin" doctor --home "$home_dir" --id "$instance_id" --json \
        >"${report_dir}/doctor-latest.json" 2>"${report_dir}/doctor-latest.err"; then
        if grep -Fq "daemon reachable" "${report_dir}/doctor-latest.json"; then
            daemon_reachable="yes"
        else
            failures=$((failures + 1))
        fi
    else
        doctor_rc=$?
        failures=$((failures + 1))
    fi

    if [ "$iterations" -eq 1 ]; then
        cp "${report_dir}/doctor-latest.json" "${report_dir}/doctor-first.json"
    fi

    if ! "$bin" policy lint --home "$home_dir" --id "$instance_id" \
        >"${report_dir}/policy-latest.out" 2>"${report_dir}/policy-latest.err"; then
        policy_rc=$?
        failures=$((failures + 1))
    fi

    printf '%s\t%s\t%s\t%s\t%s\n' \
        "$timestamp" "$pid_alive" "$doctor_rc" "$daemon_reachable" "$policy_rc" >>"$samples"

    now="$(date +%s)"
    remaining=$((deadline - now))
    if [ "$remaining" -le 0 ]; then
        break
    fi
    if [ "$remaining" -lt "$interval_secs" ]; then
        sleep "$remaining"
    else
        sleep "$interval_secs"
    fi
done

end_epoch="$(date +%s)"
elapsed_secs=$((end_epoch - start_epoch))
result="pass"
if [ "$failures" -ne 0 ]; then
    result="fail"
fi

cat <<REPORT
# Cortex Daemon Soak Report

- Generated: ${generated_at}
- Git revision: ${git_rev}
- Result: ${result}
- Classification: ${classification}
- Requested duration: ${duration} (${duration_secs}s)
- Elapsed duration: ${elapsed_secs}s
- Sample interval: ${interval} (${interval_secs}s)
- Samples: ${iterations}
- Failures: ${failures}
- Instance id: ${instance_id}
- Report directory: ${report_dir}

## Artifacts

- daemon log: ${report_dir}/daemon.log
- sample table: ${report_dir}/samples.tsv
- first doctor JSON: ${report_dir}/doctor-first.json
- latest doctor JSON: ${report_dir}/doctor-latest.json
- latest policy lint output: ${report_dir}/policy-latest.out

## Boundary

Runs shorter than 24h are smoke evidence only. Do not record them as 24h,
72h, or 7d soak evidence in release notes or status.
REPORT

if [ "$failures" -ne 0 ]; then
    exit 1
fi
