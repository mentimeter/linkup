#!/bin/bash

set -e

CHANNEL="stable"
GITHUB_API="https://api.github.com/repos/mentimeter/linkup"

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --channel=*) CHANNEL="${1#*=}"; shift ;;
            --channel)   CHANNEL="$2"; shift 2 ;;
            -h|--help)   echo "Usage: $0 [--channel stable|beta]"; exit 0 ;;
            *) echo "Unknown argument: $1" >&2; exit 1 ;;
        esac
    done

    [[ "$CHANNEL" == "stable" || "$CHANNEL" == "beta" ]] \
        || { echo "Invalid channel: '$CHANNEL'. Must be 'stable' or 'beta'." >&2; exit 1; }
}

check_dependencies() {
    command -v linkup >/dev/null 2>&1 && { echo "Linkup is already installed. To update it, run 'linkup update'."; exit 0; }
    command -v curl >/dev/null 2>&1 || { echo "curl is required." >&2; exit 1; }
    command -v cloudflared >/dev/null 2>&1 || {
        echo "WARN: 'cloudflared' is not installed. Please install it before installing Linkup." >&2
        echo "More info: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/" >&2
        exit 1
    }
}

detect_target() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-gnu" ;;
        *) echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

gh_api() {
    curl -fsSL \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$1"
}

fetch_beta_download_url() {
    local target="$1"

    command -v jq >/dev/null 2>&1 || { echo "jq is required for installing from beta channel." >&2; exit 1; }

    echo "Looking for the latest beta version..." >&2
    local releases
    releases=$(gh_api "${GITHUB_API}/releases")

    local url
    url=$(echo "$releases" \
        | jq -r --arg target "$target" \
            '[.[] | select(.prerelease)] | first | .assets[] | select((.name | contains($target)) and (.name | endswith(".tar.gz"))) | .browser_download_url // empty')

    if [ -z "$url" ]; then
        echo "No pre-releases found. Falling back to latest stable release." >&2
        fetch_stable_download_url "$target"
        return
    fi

    local release_tag
    release_tag=$(echo "$releases" | jq -r '[.[] | select(.prerelease)] | first | .tag_name')
    echo "Found pre-release version: ${release_tag}" >&2

    echo "$url"
}

fetch_stable_download_url() {
    local target="$1"

    gh_api "${GITHUB_API}/releases/latest" \
        | grep -o '"browser_download_url": *"[^"]*'"${target}"'[^"]*\.tar\.gz"' \
        | head -1 \
        | grep -o 'https://[^"]*'
}

install_binary() {
    local download_url="$1"
    local target="$2"
    local install_dir

    [ "$(uname -s)" = "Darwin" ] && install_dir="$HOME/.linkup/bin" || install_dir="/usr/local/bin"

    echo "Downloading linkup for ${target}..." >&2

    local tmp
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    curl -fsSL "$download_url" | tar -xz -C "$tmp"

    if [ "$(uname -s)" = "Darwin" ]; then
        mkdir -p "$install_dir"
        mv "$tmp/linkup" "$install_dir/linkup"
        chmod 755 "$install_dir/linkup"
    else
        sudo mv "$tmp/linkup" "$install_dir/linkup"
        sudo chmod 755 "$install_dir/linkup"
        sudo setcap cap_net_bind_service=+ep "$install_dir/linkup"
    fi

    echo "Linkup installed at ${install_dir}/linkup" >&2
    echo "$install_dir"
}

print_path_hint() {
    local install_dir="$1"

    case ":$PATH:" in
        *":$install_dir:"*) return ;;
    esac

    printf '\nAdd '"'"'%s'"'"' to your PATH.\n' "$install_dir"
    case "$(basename "${SHELL:-}")" in
        bash) printf "  echo 'export PATH=\$PATH:%s' >> ~/.bashrc\n" "$install_dir" ;;
        zsh)  printf "  echo 'export PATH=\$PATH:%s' >> ~/.zshrc\n" "$install_dir" ;;
        fish) printf "  echo 'set -gx PATH \$PATH %s' >> ~/.config/fish/config.fish\n" "$install_dir" ;;
    esac
    printf 'Then restart your shell.\n'
}

main() {
    parse_args "$@"
    check_dependencies

    local target
    target=$(detect_target)

    local download_url
    if [ "$CHANNEL" = "beta" ]; then
        download_url=$(fetch_beta_download_url "$target")
    else
        download_url=$(fetch_stable_download_url "$target")
    fi

    if [ -z "$download_url" ]; then
        echo "Could not find a release asset for ${target}." >&2
        exit 1
    fi

    local install_dir
    install_dir=$(install_binary "$download_url" "$target")

    print_path_hint "$install_dir"

    echo ""
    echo "Linkup installation complete!"
}

main "$@"
