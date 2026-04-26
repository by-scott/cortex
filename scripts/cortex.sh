#!/usr/bin/env bash
# Cortex release binary installer.
#
# One-line install:
#   curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
#
# This script only installs or updates the published binary. Cortex 1.5.0 does
# not ship the old daemon/systemd/channel setup flow.

set -euo pipefail

REPO="by-scott/cortex"
INSTALL_DIR="${CORTEX_INSTALL_DIR:-$HOME/.local/bin}"
CORTEX_HOME="${CORTEX_HOME:-$HOME/.cortex}"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"

info() { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
error() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
ok() { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }

detect_platform() {
    local os
    local arch
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    arch="$(uname -m)"

    if [ "$os" != "linux" ]; then
        error "No prebuilt Cortex binary is published for ${os}; build from source instead."
        exit 1
    fi

    case "$arch" in
        x86_64|amd64) PLATFORM="linux-amd64" ;;
        *)
            error "No prebuilt Cortex binary is published for architecture ${arch}; build from source instead."
            exit 1
            ;;
    esac
}

resolve_version() {
    local requested="${1:-latest}"
    if [ "$requested" = "latest" ]; then
        VERSION="$(curl -sSf "${GITHUB_API}/latest" \
            | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' \
            | head -1)"
        if [ -z "${VERSION:-}" ]; then
            error "Failed to resolve latest Cortex release"
            exit 1
        fi
    else
        VERSION="${requested#v}"
    fi
    info "Version: v${VERSION}"
}

release_asset_url() {
    local asset_name="$1"
    curl -sSf "${GITHUB_API}/tags/v${VERSION}" \
        | grep "browser_download_url" \
        | grep "${asset_name}" \
        | head -1 \
        | sed 's/.*"\(https[^"]*\)".*/\1/'
}

download_binary() {
    local asset_name="cortex-v${VERSION}-${PLATFORM}.tar.gz"
    local checksum_name="${asset_name}.sha256"
    local asset_url
    local checksum_url
    local expected
    local tmpdir

    asset_url="$(release_asset_url "$asset_name" || true)"
    checksum_url="$(release_asset_url "$checksum_name" || true)"
    if [ -z "${asset_url:-}" ]; then
        error "Release v${VERSION} has no ${asset_name} asset"
        exit 1
    fi
    if [ -z "${checksum_url:-}" ]; then
        error "Release v${VERSION} has no ${checksum_name} asset"
        exit 1
    fi

    tmpdir="$(mktemp -d)"
    trap "rm -rf '$tmpdir'" EXIT

    info "Downloading ${asset_name}"
    curl -sSfL "$asset_url" -o "${tmpdir}/${asset_name}"
    curl -sSfL "$checksum_url" -o "${tmpdir}/${checksum_name}"

    expected="$(awk '{print $1}' "${tmpdir}/${checksum_name}")"
    if [ -z "$expected" ]; then
        error "Checksum file is empty or malformed"
        exit 1
    fi
    printf '%s  %s\n' "$expected" "$asset_name" \
        | (cd "$tmpdir" && sha256sum -c - >/dev/null)

    tar xzf "${tmpdir}/${asset_name}" -C "$tmpdir"
    if [ ! -f "${tmpdir}/cortex" ]; then
        error "Downloaded archive does not contain a cortex binary"
        exit 1
    fi

    mkdir -p "$INSTALL_DIR"
    install -m 755 "${tmpdir}/cortex" "${INSTALL_DIR}/cortex"
    rm -rf "$tmpdir"
    trap - EXIT
    ok "Installed ${INSTALL_DIR}/cortex"
}

ensure_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac
    warn "${INSTALL_DIR} is not in PATH"
    echo "Add this to your shell profile:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
}

cmd_install() {
    local version="latest"
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                version="$2"
                shift 2
                ;;
            *)
                error "Unknown install option: $1"
                exit 1
                ;;
        esac
    done
    detect_platform
    resolve_version "$version"
    download_binary
    ensure_path
}

cmd_uninstall() {
    local purge=false
    while [ $# -gt 0 ]; do
        case "$1" in
            --purge) purge=true; shift ;;
            *) error "Unknown uninstall option: $1"; exit 1 ;;
        esac
    done
    rm -f "${INSTALL_DIR}/cortex"
    if "$purge"; then
        rm -rf "$CORTEX_HOME"
    fi
    ok "Uninstalled Cortex"
}

run_installed() {
    if [ ! -x "${INSTALL_DIR}/cortex" ]; then
        error "Cortex is not installed at ${INSTALL_DIR}/cortex"
        exit 1
    fi
    "${INSTALL_DIR}/cortex" "$@"
}

print_help() {
    cat <<EOF
Cortex release installer

Usage: cortex.sh <command> [options]

Commands:
  install [--version X.Y.Z]   Install the latest or specified release binary
  update  [--version X.Y.Z]   Alias for install
  uninstall [--purge]         Remove the installed binary; --purge removes ${CORTEX_HOME}
  status                      Run the installed 'cortex status'
  version                     Run the installed 'cortex version'
  help                        Show this help

One-line install:
  curl -sSf https://raw.githubusercontent.com/${REPO}/main/scripts/cortex.sh | bash -s -- install
EOF
}

case "${1:-help}" in
    install) shift; cmd_install "$@" ;;
    update) shift; cmd_install "$@" ;;
    uninstall) shift; cmd_uninstall "$@" ;;
    status) run_installed status ;;
    version) run_installed version ;;
    help|-h|--help) print_help ;;
    *)
        error "Unknown command: $1"
        print_help
        exit 1
        ;;
esac
