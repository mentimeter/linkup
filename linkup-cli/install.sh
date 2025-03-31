#!/bin/sh

command_exists() {
    command -v -- "$1" >/dev/null 2>&1
}

check_dependencies() {
    if ! command_exists "cloudflared"; then
        printf '%s\n' "WARN: 'cloudflared' is not installed. Please install it before installing Linkup." 1>&2
        printf '%s\n' "For more info check: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/" 1>&2
        exit 1
    fi
}

detect_platform() {
    OS=$(uname -s)
    ARCH=$(uname -m)

    case "$OS" in
    Darwin*)
        FETCH_OS='apple-darwin'
        case "$ARCH" in
        arm64 | aarch64)
            FETCH_ARCH='aarch64'
            ;;
        x86_64)
            FETCH_ARCH='x86_64'
            ;;
        esac
        ;;
    Linux*)
        FETCH_OS='unknown-linux-gnu'
        case "$ARCH" in
        arm64 | aarch64)
            FETCH_ARCH='aarch64'
            ;;
        x86_64)
            FETCH_ARCH='x86_64'
            ;;
        esac
        ;;
    esac

    if [ -z "$FETCH_OS" ] || [ -z "$FETCH_ARCH" ]; then
        printf '%s\n' "Unsupported OS/Arch combination: $OS/$ARCH" 1>&2
        exit 1
    fi
}

get_release_data() {
    if [ "$INSTALL_PRERELEASE" -eq 1 ]; then
        printf '%s\n' "Looking for the latest pre-release version..." 1>&2

        RELEASES_JSON=$(
            curl -sL \
                -H "Accept: application/vnd.github+json" \
                -H "X-GitHub-Api-Version: 2022-11-28" \
                "https://api.github.com/repos/mentimeter/linkup/releases"
        )

        RELEASE_DATA=$(echo "$RELEASES_JSON" | jq -r '[.[] | select(.prerelease==true)][0]')

        if [ "$RELEASE_DATA" = "null" ] || [ -z "$RELEASE_DATA" ]; then
            printf '%s\n' "No pre-releases found. Falling back to latest stable release." 1>&2
            get_latest_stable_release
        else
            RELEASE_TAG=$(echo "$RELEASE_DATA" | jq -r '.tag_name')
            printf '%s\n' "Found pre-release version: $RELEASE_TAG" 1>&2
        fi
    else
        get_latest_stable_release
    fi
}

get_latest_stable_release() {
    RELEASE_DATA=$(
        curl -sL \
            -H "Accept: application/vnd.github+json" \
            -H "X-GitHub-Api-Version: 2022-11-28" \
            "https://api.github.com/repos/mentimeter/linkup/releases/latest"
    )
}

download_and_extract() {
    ASSET_FILTER="linkup-.+-$FETCH_ARCH-$FETCH_OS\\.tar\\.gz$"
    FILE_DOWNLOAD_URL=$(echo "$RELEASE_DATA" | jq -r --arg filter "$ASSET_FILTER" '.assets[] | select(.name | test($filter)) | .browser_download_url')

    if [ -z "$FILE_DOWNLOAD_URL" ]; then
        printf '%s\n' "Could not find file with pattern 'linkup-*-$FETCH_ARCH-$FETCH_OS.tar.gz' in the GitHub release." 1>&2
        exit 1
    fi

    printf '%s\n' "Downloading: $FILE_DOWNLOAD_URL" 1>&2
    curl -sLO --output-dir "/tmp" "$FILE_DOWNLOAD_URL"

    LOCAL_FILE_PATH="/tmp/$(basename "$FILE_DOWNLOAD_URL")"

    printf '%s\n' "Decompressing $LOCAL_FILE_PATH" 1>&2
    tar -xzf "$LOCAL_FILE_PATH" -C /tmp

    mkdir -p "$HOME/.linkup/bin"
    mv /tmp/linkup "$HOME/.linkup/bin/"
    chmod +x "$HOME/.linkup/bin/linkup"
    printf '%s\n' "Linkup installed on $HOME/.linkup/bin/linkup" 1>&2

    rm "$LOCAL_FILE_PATH"
}

setup_path() {
    case ":$PATH:" in
    *":$HOME/.linkup/bin:"*)
        # PATH already contains the directory
        ;;
    *)
        SHELL_NAME=$(basename "$SHELL")
        case "$SHELL_NAME" in
        bash)
            PROFILE_FILE="$HOME/.bashrc"
            ;;
        zsh)
            PROFILE_FILE="$HOME/.zshrc"
            ;;
        fish)
            PROFILE_FILE="$HOME/.config/fish/config.fish"
            ;;
        *)
            PROFILE_FILE="$HOME/.profile"
            ;;
        esac

        printf '%s\n' "Adding Linkup bin to PATH in $PROFILE_FILE" 1>&2
        printf "\n# Linkup bin\nexport PATH=\$PATH:\$HOME/.linkup/bin" >>"$PROFILE_FILE"
        printf '%s\n' "Please source your profile file or restart your terminal to apply the changes." 1>&2
        ;;
    esac
}

parse_arguments() {
    while [ $# -gt 0 ]; do
        case "$1" in
        --pre-release | -p)
            INSTALL_PRERELEASE=1
            shift
            ;;
        *)
            printf '%s\n' "Unknown option: $1" 1>&2
            printf '%s\n' "Usage: ./install.sh [--pre-release|-p]" 1>&2
            exit 1
            ;;
        esac
    done
}

#-------------------------------------------------
# Main script
#-------------------------------------------------

INSTALL_PRERELEASE=${INSTALL_PRERELEASE:-0}
FETCH_OS=''
FETCH_ARCH=''
RELEASE_DATA=''

main() {
    parse_arguments "$@"

    if command_exists "linkup"; then
        printf '%s\n' "Linkup is already installed. To update it, run 'linkup update'." 1>&2
        exit 0
    fi

    check_dependencies
    detect_platform
    get_release_data
    download_and_extract
    setup_path

    printf '%s\n' "Linkup installation complete! 🎉" 1>&2
}

main "$@"
