if command -v -- "linkup" >/dev/null 2>&1; then
    echo "Linkup is already installed. To update it, run 'linkup update'."
    exit 0
fi

# region: Dependencies
# TODO: Maybe we want this script to be able to install the dependencies as well?
if ! command -v -- "cloudflared" >/dev/null 2>&1; then
    echo "WARN: 'cloudflared' is not installed. Some features will not work as expected. Please install it.\nFor more info check: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/"
fi

if ! command -v -- "caddy" >/dev/null 2>&1; then
    echo "WARN: 'caddy' is not installed. Some features will not work as expected. Please install it.\nFor more info check: https://caddyserver.com/docs/install"
fi

if ! command -v -- "dnsmasq" >/dev/null 2>&1; then
    echo "WARN: 'dnsmasq' is not installed. Some features will not work as expected. Please install it.\nFor more info check: https://thekelleys.org.uk/dnsmasq/doc.html"
fi
# endregion: Dependencies

OS=$(uname -s)
ARCH=$(uname -m)

FETCH_OS=''
FETCH_ARCH=''
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
    echo "Unsupported OS/Arch combination: $OS/$ARCH"
    exit 1
fi

LOOKUP_FILE_DOWNLOAD_URL="https://github.com/mentimeter/linkup/releases/download/.*/linkup-.*-$FETCH_ARCH-$FETCH_OS.tar.gz"
FILE_DOWNLOAD_URL=$(
    curl -sL \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        https://api.github.com/repos/mentimeter/linkup/releases/latest |
        grep -Eio "$LOOKUP_FILE_DOWNLOAD_URL"
)

if [ -z "$FILE_DOWNLOAD_URL" ]; then
    echo "Could not find file with pattern '$LOOKUP_FILE_DOWNLOAD_URL' on the latest GitHub release."
    exit 1
fi

echo "Downloading: $FILE_DOWNLOAD_URL"
curl -sLO --output-dir "/tmp" $FILE_DOWNLOAD_URL

LOCAL_FILE_PATH="/tmp/$(basename $FILE_DOWNLOAD_URL)"

echo "Decompressing $LOCAL_FILE_PATH"
tar -xzf $LOCAL_FILE_PATH -C /tmp

mkdir -p $HOME/.linkup/bin
mv /tmp/linkup $HOME/.linkup/bin/
echo "Linkup installed on $HOME/.linkup/bin/linkup"

rm "$LOCAL_FILE_PATH"

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

    echo "Adding Linkup bin to PATH in $PROFILE_FILE"
    echo -e "\n# Linkup bin\nexport PATH=\$PATH:\$HOME/.linkup/bin" >>"$PROFILE_FILE"
    echo "Please source your profile file or restart your terminal to apply the changes."
    ;;
esac
