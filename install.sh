#!/bin/sh
set -eu

REPO="coralogix/cx-cli"
BINARY_NAME="cx"
TMPDIR_CLEANUP=""

main() {
    need_cmd curl || need_cmd wget

    local os arch target version install_dir

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-musl" ;;
        MINGW*|MSYS*|CYGWIN*)
            err "Windows is not supported by this installer. Download the binary from:"
            err "  https://github.com/${REPO}/releases"
            exit 1
            ;;
        *) err "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) err "Unsupported architecture: $arch"; exit 1 ;;
    esac

    target="${arch}-${os}"

    if [ -n "${CX_VERSION:-}" ]; then
        version="$CX_VERSION"
    else
        say "Fetching latest version..."
        version="$(get_latest_version)"
        if [ -z "$version" ]; then
            err "Could not determine the latest ${BINARY_NAME} version from GitHub."
            err "Retry later, or pin a version explicitly, e.g.:"
            err "  curl -fsSL https://get.coralogix.dev/cli | CX_VERSION=<version> sh"
            err "Available versions: https://github.com/${REPO}/releases"
            exit 1
        fi
    fi

    say "Installing ${BINARY_NAME} v${version} (${target})..."

    TMPDIR_CLEANUP="$(mktemp -d)"
    trap 'rm -rf "$TMPDIR_CLEANUP"' EXIT

    local archive_name="cx-${version}-${target}.tar.gz"
    local archive_url="https://github.com/${REPO}/releases/download/v${version}/${archive_name}"
    local checksums_url="https://github.com/${REPO}/releases/download/v${version}/checksums-sha256.txt"

    say "Downloading ${archive_url}..."
    download "$archive_url" "${TMPDIR_CLEANUP}/${archive_name}"
    download "$checksums_url" "${TMPDIR_CLEANUP}/checksums-sha256.txt"

    say "Verifying checksum..."
    verify_checksum "${TMPDIR_CLEANUP}" "${archive_name}"

    say "Extracting..."
    tar xzf "${TMPDIR_CLEANUP}/${archive_name}" -C "${TMPDIR_CLEANUP}"

    install_dir="${CX_INSTALL_DIR:-}"
    if [ -z "$install_dir" ]; then
        if [ -w "/usr/local/bin" ]; then
            install_dir="/usr/local/bin"
        elif [ -d "$HOME/.local/bin" ]; then
            install_dir="$HOME/.local/bin"
        else
            mkdir -p "$HOME/.local/bin"
            install_dir="$HOME/.local/bin"
        fi
    fi

    if [ -w "$install_dir" ]; then
        cp "${TMPDIR_CLEANUP}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
    else
        say "Elevated permissions required to install to ${install_dir}"
        sudo cp "${TMPDIR_CLEANUP}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
    fi
    chmod +x "${install_dir}/${BINARY_NAME}"

    # Write install-method marker so cx can suggest the right upgrade command.
    local cx_dir
    cx_dir="${CX_HOME:-$HOME}/.cx"
    mkdir -p "$cx_dir"
    printf 'curl' > "${cx_dir}/install-method"

    say "Installed ${BINARY_NAME} to ${install_dir}/${BINARY_NAME}"

    case ":$PATH:" in
        *":${install_dir}:"*) ;;
        *)
            warn "${install_dir} is not in your PATH."
            warn "Add it by running:  export PATH=\"${install_dir}:\$PATH\""
            ;;
    esac

    say "Done! Run '${BINARY_NAME} --help' to get started."
}

# Resolve the latest release by following the github.com/<repo>/releases/latest
# redirect to /releases/tag/v<version>. This avoids api.github.com, whose
# unauthenticated rate limit (60 req/hour per IP) breaks installs behind
# shared NAT/egress IPs. Uses a plain GET (not HEAD) because some corporate
# proxies block HEAD requests; the response body is discarded.
get_latest_version() {
    local url="https://github.com/${REPO}/releases/latest"
    local final_url
    if command -v curl > /dev/null 2>&1; then
        final_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$url" || true)"
    else
        final_url="$(wget -S -O /dev/null "$url" 2>&1 | tr -d '\r' \
            | sed -n 's/^ *Location: *\([^ ]*\).*/\1/p' | tail -n 1 || true)"
    fi
    printf '%s' "$final_url" | sed -n 's|.*/releases/tag/v\([^/?#]*\).*|\1|p'
}

download() {
    local url="$1" dest="$2"
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    else
        wget -qO "$dest" "$url"
    fi
}

verify_checksum() {
    local dir="$1" file="$2"
    local expected actual
    expected="$(grep "$file" "${dir}/checksums-sha256.txt" | cut -d ' ' -f 1)"
    if [ -z "$expected" ]; then
        err "Checksum not found for ${file}"
        exit 1
    fi
    if command -v sha256sum > /dev/null 2>&1; then
        actual="$(sha256sum "${dir}/${file}" | cut -d ' ' -f 1)"
    elif command -v shasum > /dev/null 2>&1; then
        actual="$(shasum -a 256 "${dir}/${file}" | cut -d ' ' -f 1)"
    else
        warn "No SHA256 tool found, skipping checksum verification"
        return 0
    fi
    if [ "$expected" != "$actual" ]; then
        err "Checksum mismatch!"
        err "  Expected: ${expected}"
        err "  Actual:   ${actual}"
        exit 1
    fi
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        return 1
    fi
}

say() {
    printf '%s\n' "$*"
}

warn() {
    printf 'Warning: %s\n' "$*" >&2
}

err() {
    printf 'Error: %s\n' "$*" >&2
}

main "$@"
