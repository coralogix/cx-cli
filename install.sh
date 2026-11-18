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

    # A Homebrew link here (common on Intel Macs, where /usr/local/bin is
    # Homebrew's bin) points into Homebrew's own files. Copying over it would
    # change Homebrew's copy behind its back, so stop and let the user choose.
    if [ -L "${install_dir}/${BINARY_NAME}" ] && is_brew_path "${install_dir}/${BINARY_NAME}"; then
        note ""
        note "${BINARY_NAME} is already installed with Homebrew at ${install_dir}/${BINARY_NAME}."
        note "This script does not replace a Homebrew install. Run one of these:"
        note ""
        options "brew uninstall cx && curl -fsSL https://get.coralogix.dev/cli | sh" "preferred: switch to this installer" \
            "brew upgrade cx" "or keep Homebrew and update it"
        exit 1
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

    # Another cx earlier on PATH (often a Homebrew install) hides this one:
    # running `cx` would still start the old copy. Never report plain success
    # in that case.
    local installed first
    installed="${install_dir}/${BINARY_NAME}"
    first="$(first_on_path "$BINARY_NAME" || true)"
    if [ -n "$first" ] && ! [ "$first" -ef "$installed" ]; then
        warn_shadowed "$installed" "$first"
        say "Then open a new shell and run '${BINARY_NAME} --version'."
        return 0
    fi

    say "Done! Run '${BINARY_NAME} --help' to get started."
}

# Print the first executable named $1 on PATH, the one a new shell runs.
# Empty PATH entries mean the current directory.
first_on_path() {
    local IFS=: dir
    for dir in $PATH; do
        [ -n "$dir" ] || dir="."
        if [ -f "${dir}/$1" ] && [ -x "${dir}/$1" ]; then
            printf '%s\n' "${dir}/$1"
            return 0
        fi
    done
    return 1
}

# True when $1 is in Homebrew's Cellar or is a link into it. Homebrew's bin/
# entries are links into the Cellar, so a plain file that this script put in
# a Homebrew folder is not mistaken for a Homebrew install.
# Keep in sync with is_brew_path in src/install_method.rs.
is_brew_path() {
    local target
    case "$1" in
        */Cellar/*) return 0 ;;
    esac
    target="$(readlink "$1" 2>/dev/null || true)"
    case "$target" in
        *Cellar/*) return 0 ;;
    esac
    return 1
}

warn_shadowed() {
    local installed="$1" first="$2" first_version
    first_version="$(CX_NO_UPDATE_NOTIFIER=1 "$first" --version 2>/dev/null | head -n 1 || true)"
    note ""
    note "Another ${BINARY_NAME} is already installed at ${first}${first_version:+ (${first_version})}, and your shell runs it first."
    note "To use the new ${BINARY_NAME}, run one of these:"
    note ""
    if is_brew_path "$first"; then
        options "brew uninstall cx" "preferred: keep the new ${installed}" \
            "brew upgrade cx && rm '${installed}'" "or keep Homebrew"
    else
        options "rm '${first}'" "remove the other ${BINARY_NAME}" \
            "export PATH=\"$(dirname "$installed"):\$PATH\"" "or add this line to your shell profile"
    fi
    note ""
}

# A message line for the user, on stderr, with no prefix.
note() {
    printf '%s\n' "$*" >&2
}

# Two commands the user can copy, each with a short comment.
# Arguments: command1 comment1 command2 comment2. Comments line up.
options() {
    local width="${#1}"
    if [ "${#3}" -gt "$width" ]; then width="${#3}"; fi
    printf "  %-${width}s  # %s\n" "$1" "$2" "$3" "$4" >&2
}

# Follow the releases/latest redirect instead of api.github.com (60 req/h per IP).
# Plain GET, not HEAD: some proxies block HEAD.
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
