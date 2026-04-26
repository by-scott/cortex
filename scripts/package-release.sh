#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
PLATFORM="${CORTEX_PACKAGE_PLATFORM:-linux-amd64}"
DIST_DIR="${CORTEX_DIST_DIR:-dist}"
ASSET_NAME="cortex-v${VERSION}-${PLATFORM}.tar.gz"

if [ -z "$VERSION" ]; then
    echo "error: workspace version is missing" >&2
    exit 1
fi

cargo build --release --bin cortex

mkdir -p "$DIST_DIR"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

install -m 755 target/release/cortex "$tmpdir/cortex"
install -m 644 README.md "$tmpdir/README.md"
install -m 644 README.zh.md "$tmpdir/README.zh.md"
install -m 644 LICENSE "$tmpdir/LICENSE"

tar \
    --sort=name \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    --mtime="@${SOURCE_DATE_EPOCH:-0}" \
    -czf "${DIST_DIR}/${ASSET_NAME}" \
    -C "$tmpdir" \
    .

sha256sum "${DIST_DIR}/${ASSET_NAME}" >"${DIST_DIR}/${ASSET_NAME}.sha256"

printf '%s\n' "${DIST_DIR}/${ASSET_NAME}"
printf '%s\n' "${DIST_DIR}/${ASSET_NAME}.sha256"
