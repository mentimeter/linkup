if [ -x "$(command -v linkup)" ]; then
    echo "Linkup is already installed. To update it, run 'linkup update'."
    exit 0
fi

# region: Dependencies
# TODO: Maybe we want this script to be able to install the dependencies as well?
if [ ! -x "$(command -v caddy)" ]; then
    echo "could not find 'caddy'. Please install it first."
    exit 1
fi

if [ ! -x "$(command -v cloudflared)" ]; then
    echo "could not find 'cloudflared'. Please install it first."
    exit 1
fi
# endregion: Dependencies

OS=$(uname -s)
ARCH=$(uname -m)

FETCH_OS=''
FETCH_ARCH=''
if [[ "$OS" == "Darwin"* ]]; then
    FETCH_OS='apple-darwin'

    if [[ "$ARCH" == "arm64" ]]; then
        FETCH_ARCH='aarch64'
    elif [[ "$arch" == "x86_64" ]]; then
        FETCH_ARCH='x86_64'
    fi
elif [[ "$OS" == "Linux"* ]]; then
    FETCH_OS='unknown-linux'

    if [[ "$ARCH" == "arm64" ]]; then
        FETCH_ARCH='aarch64'
    elif [[ "$arch" == "x86_64" ]]; then
        FETCH_ARCH='x86_64'
    fi
fi

if [[ -z "$FETCH_OS" || -z "$FETCH_ARCH" ]]; then
    echo "Unsupported OS/Arch combination: $OS/$ARCH"
    exit 1
fi

LOOKUP_FILE_DOWNLOAD_URL="https://github.com/mentimeter/linkup/releases/download/.*/linkup-.*-$FETCH_ARCH-$FETCH_OS.tar.gz"
FILE_DOWNLOAD_URL=$(curl -sL \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  https://api.github.com/repos/mentimeter/linkup/releases/latest \
  | grep -Eio "$LOOKUP_FILE_DOWNLOAD_URL"
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

if [[ ":$PATH:" != *":$HOME/.linkup/bin:"* ]]; then
    # TODO: Can we do this on a better way to ensure we support more different shell?
    # Check if is ZSH and add the bin to path if so.
    if [ -n "${ZSH_VERSION-}" ]; then
        echo "\n# Linkup bin\nexport PATH=\$PATH:\$HOME/.linkup/bin" >> $HOME/.zshrc
    else
        echo "Add the following to your shell and source it again:\n\texport PATH=\$PATH:\$HOME/.linkup/bin"
    fi
fi
