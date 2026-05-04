#!/usr/bin/env bash
# Cortex — Installer & Management Script
#
# One-line install:
#   curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
#   curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
#     CORTEX_PERMISSION_LEVEL=open bash -s -- install --permission-level open
# Default install permission mode is balanced when not specified.
#
# Commands:
#   install   [--version X.Y.Z] [cortex install args...]
#             Download verified binary + run `cortex install` with the remaining args
#   uninstall [--purge]           Remove binary + service (--purge removes data)
#   update    [--version X.Y.Z]  Upgrade to latest or specified version
#   status                        Show daemon status
#   bench                         Run Criterion benchmarks
#   version                       Show installed version

set -euo pipefail

REPO="by-scott/cortex"
INSTALL_DIR="${CORTEX_INSTALL_DIR:-$HOME/.local/bin}"
CORTEX_HOME="${CORTEX_HOME:-$HOME/.cortex}"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"
SUPPORTED_PREBUILT_PLATFORM="linux-amd64"

# ── Logging ─────────────────────────────────────────────────

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
error() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }

# ── Platform detection ──────────────────────────────────────

detect_platform() {
    local os arch
    if [ -n "${CORTEX_INSTALL_PLATFORM:-}" ]; then
        PLATFORM="${CORTEX_INSTALL_PLATFORM}"
        return
    fi

    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    arch="$(uname -m)"

    case "$os" in
        linux)  os="linux" ;;
        darwin) os="macos" ;;
        *)      error "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="amd64" ;;
        aarch64|arm64)  arch="arm64" ;;
        *)              error "Unsupported architecture: $arch"; exit 1 ;;
    esac

    PLATFORM="${os}-${arch}"
}

ensure_prebuilt_platform() {
    if [ "$PLATFORM" = "$SUPPORTED_PREBUILT_PLATFORM" ]; then
        return
    fi

    error "No official prebuilt installer binary is published for ${PLATFORM}"
    echo ""
    echo "Supported prebuilt platform: ${SUPPORTED_PREBUILT_PLATFORM}"
    if [ -n "${CORTEX_INSTALL_PLATFORM:-}" ]; then
        echo "CORTEX_INSTALL_PLATFORM only changes installer platform selection; it does"
        echo "not create missing release assets."
    fi
    echo ""
    echo "Options:"
    echo "  1. Build with Docker:"
    echo "     git clone https://github.com/${REPO}.git && cd cortex"
    echo "     docker compose run --rm dev cargo build --release"
    echo ""
    echo "  2. Build from source:"
    echo "     git clone https://github.com/${REPO}.git && cd cortex"
    echo "     cargo build --release"
    echo ""
    exit 1
}

# ── Checksums ───────────────────────────────────────────────

verify_checksum() {
    local archive_path="$1"
    local checksum_path="$2"
    local asset_name="$3"
    local expected actual

    expected="$(awk '{print $1}' "$checksum_path" | head -1)"
    if [ -z "$expected" ]; then
        error "Checksum file is empty: ${checksum_path}"
        exit 1
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$archive_path" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
    else
        error "Neither sha256sum nor shasum is available for checksum verification"
        exit 1
    fi

    if [ "$actual" != "$expected" ]; then
        error "Checksum mismatch for ${asset_name}"
        echo "  expected: ${expected}"
        echo "  actual:   ${actual}"
        exit 1
    fi

    ok "Checksum verified: ${asset_name}"
}

release_asset_url() {
    local release_json="$1"
    local asset_name="$2"

    printf '%s\n' "$release_json" \
        | grep '"browser_download_url"' \
        | grep -F "/${asset_name}\"" \
        | head -1 \
        | sed 's/.*"\(https[^"]*\)".*/\1/'
}

# ── GitHub Release download ─────────────────────────────────

resolve_version() {
    local version="${1:-latest}"
    if [ "$version" = "latest" ]; then
        VERSION=$(curl -sSf "${GITHUB_API}/latest" 2>/dev/null \
            | grep '"tag_name"' | head -1 | sed 's/.*"v\([^"]*\)".*/\1/' ) || true
        if [ -z "${VERSION:-}" ]; then
            error "Failed to fetch latest version from GitHub"
            exit 1
        fi
    else
        VERSION="${version#v}"
    fi
    info "Version: v${VERSION}"
}

download_binary() {
    local asset_name="cortex-v${VERSION}-${PLATFORM}.tar.gz"
    local checksum_name="${asset_name}.sha256"
    local release_api="${GITHUB_API}/tags/v${VERSION}"
    local release_json download_url checksum_url
    local tmpdir
    local binary_path

    ensure_prebuilt_platform

    info "Looking for ${asset_name} in v${VERSION}..."
    release_json="$(curl -sSf "$release_api" 2>/dev/null)" || {
        error "Failed to fetch release metadata for v${VERSION}"
        exit 1
    }
    download_url="$(release_asset_url "$release_json" "$asset_name")" || true

    if [ -z "${download_url:-}" ]; then
        error "No prebuilt binary asset found for ${PLATFORM} in v${VERSION}: ${asset_name}"
        echo ""
        echo "The installer only installs platforms with a matching GitHub Release asset."
        echo "To inspect the published assets:"
        echo "  curl -sSf ${release_api} | grep browser_download_url"
        echo ""
        echo "Options:"
        echo "  1. Build with Docker:"
        echo "     git clone https://github.com/${REPO}.git && cd cortex"
        echo "     docker compose run --rm dev cargo build --release"
        echo ""
        echo "  2. Build from source:"
        echo "     git clone https://github.com/${REPO}.git && cd cortex"
        echo "     cargo build --release"
        echo ""
        exit 1
    fi
    checksum_url="$(release_asset_url "$release_json" "$checksum_name")" || true
    if [ -z "${checksum_url:-}" ]; then
        error "No checksum asset found for ${asset_name} in v${VERSION}: ${checksum_name}"
        echo "Refusing to install an unverified prebuilt binary."
        exit 1
    fi

    info "Downloading: ${download_url}"
    mkdir -p "$INSTALL_DIR"

    tmpdir="$(mktemp -d)"
    trap "rm -rf '$tmpdir'" EXIT
    curl -sSfL "$download_url" -o "${tmpdir}/${asset_name}"
    curl -sSfL "$checksum_url" -o "${tmpdir}/${checksum_name}"
    verify_checksum "${tmpdir}/${asset_name}" "${tmpdir}/${checksum_name}" "$asset_name"
    tar xzf "${tmpdir}/${asset_name}" -C "$tmpdir"

    binary_path="${tmpdir}/cortex"
    if [ ! -f "$binary_path" ]; then
        binary_path="$(find "$tmpdir" -mindepth 2 -maxdepth 2 -type f -name cortex | head -1 || true)"
    fi
    if [ -z "${binary_path:-}" ] || [ ! -f "$binary_path" ]; then
        error "Downloaded archive does not contain an installable cortex binary"
        exit 1
    fi

    install -m 755 "$binary_path" "${INSTALL_DIR}/cortex"
    rm -rf "$tmpdir"
    trap - EXIT

    ok "Binary installed: ${INSTALL_DIR}/cortex"
}

# ── PATH check ──────────────────────────────────────────────

ensure_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac
    warn "${INSTALL_DIR} is not in PATH"
    echo "  Add to your shell profile:"
    echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
}

# ── systemd service ─────────────────────────────────────────

setup_instance() {
    local cortex_bin="${INSTALL_DIR}/cortex"
    if [ ! -x "$cortex_bin" ]; then
        warn "Binary not found at ${cortex_bin}, skipping instance setup"
        return
    fi

    # Initialize the requested instance (generates config.toml with env vars)
    info "Initializing instance..."
    "$cortex_bin" install "$@" 2>/dev/null || {
        warn "Instance setup failed (you can do it later: cortex install $*)"
    }
}

# ── Commands ────────────────────────────────────────────────

cmd_install() {
    local version="latest"
    local -a install_args=()
    while [ $# -gt 0 ]; do
        case "$1" in
            --version) version="$2"; shift 2 ;;
            *) install_args+=("$1"); shift ;;
        esac
    done

    info "Cortex installer"
    echo ""

    detect_platform
    resolve_version "$version"
    download_binary
    ensure_path
    setup_instance "${install_args[@]}"

    echo ""
    ok "Installation complete!"
    echo ""
    echo "  Start a conversation:  cortex"
    echo "  Check daemon status:   cortex status"
    echo "  Configure:             edit ${CORTEX_HOME}/config.toml"
    echo ""
}

cmd_uninstall() {
    local purge=false
    for arg in "$@"; do
        case "$arg" in
            --purge) purge=true ;;
        esac
    done

    local cortex_bin="${INSTALL_DIR}/cortex"

    # Stop and remove service
    if command -v systemctl >/dev/null 2>&1; then
        if systemctl --user is-active cortex >/dev/null 2>&1; then
            info "Stopping cortex service..."
            systemctl --user stop cortex 2>/dev/null || true
        fi
        if [ -x "$cortex_bin" ]; then
            "$cortex_bin" uninstall 2>/dev/null || true
        fi
    fi

    # Remove binary
    if [ -f "$cortex_bin" ]; then
        info "Removing binary: ${cortex_bin}"
        rm -f "$cortex_bin"
    fi

    # Purge data
    if $purge && [ -d "$CORTEX_HOME" ]; then
        info "Removing all data: ${CORTEX_HOME}"
        rm -rf "$CORTEX_HOME"
    fi

    ok "Uninstall complete."
}

cmd_update() {
    local version="latest"
    while [ $# -gt 0 ]; do
        case "$1" in
            --version) version="$2"; shift 2 ;;
            *) shift ;;
        esac
    done

    local cortex_bin="${INSTALL_DIR}/cortex"

    # Show current version
    if [ -x "$cortex_bin" ]; then
        local current
        current=$("$cortex_bin" --version 2>/dev/null | awk '{print $2}') || true
        info "Current version: ${current:-unknown}"
    fi

    # Stop service if running
    if command -v systemctl >/dev/null 2>&1 \
       && systemctl --user is-active cortex >/dev/null 2>&1; then
        info "Stopping cortex service for update..."
        systemctl --user stop cortex 2>/dev/null || true
    fi

    detect_platform
    resolve_version "$version"
    download_binary

    # Restart service if it was running
    if command -v systemctl >/dev/null 2>&1 \
       && systemctl --user is-enabled cortex >/dev/null 2>&1; then
        info "Restarting cortex service..."
        systemctl --user start cortex 2>/dev/null || true
    fi

    ok "Updated to v${VERSION}"
}

cmd_status() {
    local cortex_bin="${INSTALL_DIR}/cortex"
    if [ -x "$cortex_bin" ]; then
        "$cortex_bin" status
    else
        error "Cortex not installed at ${cortex_bin}"
        exit 1
    fi
}

cmd_bench() {
    echo "=== Cortex Benchmark Suite ==="
    echo ""

    local benchmarks=("journal_bench" "parse_bench" "cache_bench")

    for bench in "${benchmarks[@]}"; do
        echo "--- Running: $bench ---"
        if command -v docker >/dev/null 2>&1 && [ -f "docker-compose.yml" ]; then
            docker compose run --rm dev cargo bench -p cortex-kernel --bench "$bench" 2>&1
        elif command -v cargo >/dev/null 2>&1; then
            cargo bench -p cortex-kernel --bench "$bench" 2>&1
        else
            warn "Neither docker nor cargo available for benchmarks"
            return
        fi
        echo ""
    done

    echo "=== All benchmarks complete ==="
}

cmd_version() {
    local cortex_bin="${INSTALL_DIR}/cortex"
    if [ -x "$cortex_bin" ]; then
        "$cortex_bin" --version
    else
        echo "Cortex not installed"
    fi
}

# ── Help ────────────────────────────────────────────────────

print_help() {
    echo "Cortex — Installer & Management Script"
    echo ""
    echo "Usage: cortex.sh <command> [options]"
    echo ""
    echo "Commands:"
    echo "  install   [--version X.Y.Z] [cortex install args...]"
    echo "                                 Download, verify, install, then run cortex install"
    echo "  uninstall [--purge]           Remove binary and service"
    echo "  update    [--version X.Y.Z]   Upgrade to latest or specific version"
    echo "  status                         Show daemon status"
    echo "  bench                          Run benchmarks"
    echo "  version                        Show installed version"
    echo ""
    echo "One-line install:"
    echo "  curl -sSf https://raw.githubusercontent.com/${REPO}/main/scripts/cortex.sh | bash -s -- install"
    echo "  curl -sSf https://raw.githubusercontent.com/${REPO}/main/scripts/cortex.sh | CORTEX_API_KEY=sk-... bash -s -- install --id work"
    echo ""
    echo "Release assets:"
    echo "  Official prebuilt installer platform: ${SUPPORTED_PREBUILT_PLATFORM}"
    echo "  Installs cortex-vX.Y.Z-\${platform}.tar.gz only when the release also"
    echo "  publishes the matching .sha256 checksum asset."
    echo ""
}

# ── Entry point ─────────────────────────────────────────────

case "${1:-}" in
    install)    shift; cmd_install "$@" ;;
    uninstall)  shift; cmd_uninstall "$@" ;;
    update)     shift; cmd_update "$@" ;;
    status)     cmd_status ;;
    bench)      cmd_bench ;;
    version)    cmd_version ;;
    -h|--help|"") print_help ;;
    *)
        error "Unknown command: $1"
        echo "Run 'cortex.sh --help' for usage."
        exit 1
        ;;
esac
