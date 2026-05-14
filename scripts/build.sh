#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [ "$#" -ne 0 ]; then
    echo "usage: ./scripts/build.sh" >&2
    exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker is required" >&2
    exit 1
fi

lint_suppressions="$(
    find Cargo.toml crates -type f \( -name '*.rs' -o -name '*.toml' \) \
        -not -path '*/target/*' \
        -exec grep -nE '#!?[[:space:]]*\[[[:space:]]*(allow|expect)[[:space:]]*\(|#!?[[:space:]]*\[[^]]*cfg_attr[^]]*,[[:space:]]*(allow|expect)[[:space:]]*\(' {} + || true
)"
if [ -n "$lint_suppressions" ]; then
    printf '%s\n' "$lint_suppressions" >&2
    echo "error: lint suppression attributes are not allowed" >&2
    exit 1
fi

warning_suppressions="$(
    find Cargo.toml crates -type f \( -name '*.toml' -o -name '*.rs' \) \
        -not -path '*/target/*' \
        -exec grep -nE '(^|[[:space:]])-A[[:space:]]+[A-Za-z0-9_:-]+|--cap-lints([=[:space:]]+)allow' {} + || true
)"
if [ -n "$warning_suppressions" ]; then
    printf '%s\n' "$warning_suppressions" >&2
    echo "error: warning suppression flags are not allowed" >&2
    exit 1
fi

floating_docker_base="$(
    grep -nE '^FROM[[:space:]]+[^[:space:]]+:(latest|stable)([[:space:]]|$)' Dockerfile || true
)"
if [ -n "$floating_docker_base" ]; then
    printf '%s\n' "$floating_docker_base" >&2
    echo "error: Docker base images must use an explicit version" >&2
    exit 1
fi

remote_static_assets="$(
    find static -type f \( -name '*.html' -o -name '*.js' -o -name '*.css' \) \
        -exec grep -nE 'https?://|cd[n]js|marke[d]|highligh[t][.]js|h[l]js' {} + || true
)"
if [ -n "$remote_static_assets" ]; then
    printf '%s\n' "$remote_static_assets" >&2
    echo "error: embedded dashboard must not depend on remote static assets" >&2
    exit 1
fi

docker compose build dev
docker compose run --rm dev sh -ec '
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- \
    -D warnings \
    -D clippy::pedantic \
    -D clippy::nursery
RUSTFLAGS="-D warnings" cargo test --workspace --all-features --locked
RUSTFLAGS="-D warnings" cargo build --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
'
