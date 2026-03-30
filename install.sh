#!/bin/sh
set -eu

REPO="coralogix/coralogix-cli"
BINARY_NAME="cx"

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
    fi

    say "Installing ${BINARY_NAME} v${version} (${target})..."

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    local archive_name="cx-${version}-${target}.tar.gz"
    local archive_url="https://github.com/${REPO}/releases/download/v${version}/${archive_name}"
    local checksums_url="https://github.com/${REPO}/releases/download/v${version}/checksums-sha256.txt"

    say "Downloading ${archive_url}..."
    download "$archive_url" "${tmpdir}/${archive_name}"
    download "$checksums_url" "${tmpdir}/checksums-sha256.txt"

    say "Verifying checksum..."
    verify_checksum "${tmpdir}" "${archive_name}"

    say "Extracting..."
    tar xzf "${tmpdir}/${archive_name}" -C "${tmpdir}"

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
        cp "${tmpdir}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
    else
        say "Elevated permissions required to install to ${install_dir}"
        sudo cp "${tmpdir}/${BINARY_NAME}" "${install_dir}/${BINARY_NAME}"
    fi
    chmod +x "${install_dir}/${BINARY_NAME}"

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

get_latest_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest"
    local response
    if command -v curl > /dev/null 2>&1; then
        response="$(curl -fsSL "$url")"
    else
        response="$(wget -qO- "$url")"
    fi
    printf '%s' "$response" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p'
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
