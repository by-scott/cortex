#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: ./scripts/install.sh [cortex install args]

Downloads the Cortex release artifact for this platform, installs the
extracted cortex binary, then runs `cortex install` with the supplied args.

Environment:
  CORTEX_REPO          GitHub repo, owner/name (default: by-scott/cortex)
  CORTEX_VERSION       Release tag or "latest" (default: latest)
  CORTEX_ASSET_NAME    Exact release asset name to download
  CORTEX_ASSET_URL     Exact artifact URL; skips GitHub release lookup
  CORTEX_INSTALL_DIR   Destination directory (default: ~/.local/bin)
  CORTEX_INSTALL_ARGS  Extra args prepended before script args
  GITHUB_TOKEN         Optional token for GitHub API requests

Release artifacts must be archives whose root contains a cortex binary.
Supported archive formats: .tar.gz, .tgz, .tar.xz, .zip.
USAGE
}

die() {
    echo "error: $*" >&2
    exit 1
}

cleanup_dir=""

cleanup() {
    if [ -n "$cleanup_dir" ]; then
        rm -rf "$cleanup_dir"
    fi
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

github_api() {
    local path=$1
    local url="https://api.github.com/repos/${CORTEX_REPO}/${path}"
    local args=(
        -fsSL
        -H "Accept: application/vnd.github+json"
        -H "User-Agent: cortex-install"
    )
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        args+=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
    fi
    curl "${args[@]}" "$url"
}

fetch_release_json() {
    if [ "$CORTEX_VERSION" = "latest" ]; then
        github_api "releases/latest"
        return
    fi

    local release_json
    if release_json=$(github_api "releases/tags/${CORTEX_VERSION}" 2>/dev/null); then
        printf '%s\n' "$release_json"
        return
    fi
    case "$CORTEX_VERSION" in
        v*) ;;
        *) github_api "releases/tags/v${CORTEX_VERSION}" && return ;;
    esac
    return 1
}

platform_patterns() {
    local os
    local arch
    os=$(uname -s)
    arch=$(uname -m)

    case "${os}:${arch}" in
        Linux:x86_64 | Linux:amd64)
            echo "linux-amd64 linux-x86_64 linux-x64 x86_64-unknown-linux-gnu"
            ;;
        Linux:aarch64 | Linux:arm64)
            echo "linux-arm64 linux-aarch64 aarch64-unknown-linux-gnu"
            ;;
        Darwin:x86_64)
            echo "darwin-amd64 darwin-x86_64 macos-amd64 macos-x86_64 x86_64-apple-darwin"
            ;;
        Darwin:arm64 | Darwin:aarch64)
            echo "darwin-arm64 darwin-aarch64 macos-arm64 macos-aarch64 aarch64-apple-darwin"
            ;;
        *)
            die "unsupported platform: ${os}/${arch}"
            ;;
    esac
}

is_archive_asset() {
    local name=$1
    case "$name" in
        *.tar.gz | *.tgz | *.tar.xz | *.zip) return 0 ;;
        *) return 1 ;;
    esac
}

select_asset_url() {
    local release_json=$1
    local url
    local name
    local lower
    local pattern
    local patterns

    if [ -n "${CORTEX_ASSET_NAME:-}" ]; then
        while IFS= read -r url; do
            name=${url##*/}
            if [ "$name" = "$CORTEX_ASSET_NAME" ]; then
                printf '%s\n' "$url"
                return
            fi
        done < <(release_asset_urls "$release_json")
        die "release has no asset named ${CORTEX_ASSET_NAME}"
    fi

    patterns=$(platform_patterns)
    while IFS= read -r url; do
        name=${url##*/}
        lower=${name,,}
        is_archive_asset "$lower" || continue
        case "$lower" in
            *sha256* | *checksums* | *.sig | *.asc) continue ;;
        esac
        case "$lower" in
            *cortex*) ;;
            *) continue ;;
        esac
        for pattern in $patterns; do
            case "$lower" in
                *"$pattern"*)
                    printf '%s\n' "$url"
                    return
                    ;;
            esac
        done
    done < <(release_asset_urls "$release_json")

    die "no Cortex archive asset found for $(uname -s)/$(uname -m)"
}

release_asset_urls() {
    local release_json=$1
    printf '%s\n' "$release_json" |
        grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*"' |
        sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'
}

download() {
    local url=$1
    local output=$2
    local args=(-fL --retry 3 --connect-timeout 15 -o "$output")
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        args+=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
    fi
    curl "${args[@]}" "$url"
}

extract_archive() {
    local archive=$1
    local dest=$2
    case "$archive" in
        *.zip)
            require_cmd unzip
            unzip -q "$archive" -d "$dest"
            ;;
        *.tar.gz | *.tgz | *.tar.xz)
            tar -xf "$archive" -C "$dest"
            ;;
        *)
            die "unsupported archive format: ${archive##*/}"
            ;;
    esac
}

install_binary() {
    local src=$1
    local dest_dir=$2
    local dest="${dest_dir}/cortex"

    if mkdir -p "$dest_dir" 2>/dev/null && [ -w "$dest_dir" ]; then
        install -m 0755 "$src" "$dest"
    else
        require_cmd sudo
        sudo install -d -m 0755 "$dest_dir"
        sudo install -m 0755 "$src" "$dest"
    fi
    printf '%s\n' "$dest"
}

contains_arg() {
    local needle=$1
    shift
    local arg
    for arg in "$@"; do
        [ "$arg" = "$needle" ] && return 0
    done
    return 1
}

main() {
    if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
        usage
        exit 0
    fi

    require_cmd curl
    require_cmd tar
    require_cmd install

    CORTEX_REPO=${CORTEX_REPO:-by-scott/cortex}
    CORTEX_VERSION=${CORTEX_VERSION:-latest}

    local tmp
    local archive
    local extract_dir
    local asset_url
    local release_json
    local binary
    local install_dir
    local installed_bin
    local extra_args=()
    local asset_name

    tmp=$(mktemp -d)
    cleanup_dir=$tmp
    trap cleanup EXIT
    archive="${tmp}/cortex-release"
    extract_dir="${tmp}/extract"
    mkdir -p "$extract_dir"

    if [ -n "${CORTEX_ASSET_URL:-}" ]; then
        asset_url=$CORTEX_ASSET_URL
    else
        release_json=$(fetch_release_json) || die "cannot fetch GitHub release metadata"
        asset_url=$(select_asset_url "$release_json")
    fi

    asset_name=${asset_url%%\?*}
    archive="${archive}-${asset_name##*/}"
    echo "Downloading ${asset_url}" >&2
    download "$asset_url" "$archive"
    extract_archive "$archive" "$extract_dir"

    binary="${extract_dir}/cortex"
    [ -f "$binary" ] || die "archive root must contain a cortex binary"
    chmod +x "$binary"

    if [ -n "${CORTEX_INSTALL_ARGS:-}" ]; then
        read -r -a extra_args <<<"$CORTEX_INSTALL_ARGS"
    fi

    if [ -n "${CORTEX_INSTALL_DIR:-}" ]; then
        install_dir=$CORTEX_INSTALL_DIR
    elif contains_arg "--system" "${extra_args[@]}" "$@"; then
        install_dir=/usr/local/bin
    else
        [ -n "${HOME:-}" ] || die "HOME is required unless CORTEX_INSTALL_DIR is set"
        install_dir="${HOME}/.local/bin"
    fi
    installed_bin=$(install_binary "$binary" "$install_dir")

    echo "Running ${installed_bin} install ${extra_args[*]} $*" >&2
    "$installed_bin" install "${extra_args[@]}" "$@"
}

main "$@"
