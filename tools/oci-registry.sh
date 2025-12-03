#!/usr/bin/env bash

# Helper script to set up a local OCI registry using zot.
# The script is prepared to work in local environments and the CI.
# Supports: Linux, macOS, Windows WSL and Windows mingw

# -e: Exit on error
# -u: Exit if undefined variable is used
# -o pipefail: Return failure if any command in a pipeline fails
set -euo pipefail


# ----- CONSTANTS -----
readonly OS=$(case "$(uname -s)" in
    Linux*)     echo "linux";;
    Darwin*)    echo "darwin";;
    MINGW**)    echo "windows";;
    *)          echo "unknown";;
esac)

readonly ARCH=$(case "$(uname -m)" in
    x86_64*)    echo "amd64";;
    amd64*)     echo "amd64";;
    aarch64*)   echo "arm64";;
    arm64*)     echo "arm64";;
    *)          echo "unknown";;
esac)

readonly CONFIG_FOLDER="$HOME/.zot"
readonly CONFIG_FILE="$CONFIG_FOLDER/config.json"
readonly BINARY="$CONFIG_FOLDER/zot"

if [ "$OS" = "windows" ]; then
    readonly STORAGE_PATH=$(cygpath -m "$CONFIG_FOLDER/storage")
    readonly LOG_FILE=$(cygpath -m "$CONFIG_FOLDER/zot.log")
else
    readonly STORAGE_PATH="$CONFIG_FOLDER/storage"
    readonly LOG_FILE="$CONFIG_FOLDER/zot.log"
fi


# ----- ARGUMENTS -----
OPTION="${1:-empty}"
VERSION="${VERSION:-v2.1.11}"
PORT="${PORT:-5000}"


# ----- HELPER FUNCTIONS -----
function print_usage() {
  echo "Usage: $0 [run|clean|help]"
  echo "  run   - Run zot (if not present, it will be downloaded and configured)"
  echo "  clean - Remove zot registry and all associated files"
  echo "  help  - Display this help message"
}

function install() {
    if [[ ! -d ~/.zot ]]; then
        mkdir ~/.zot
    fi

    if [[ ! -f $CONFIG_FOLDER/zot ]]; then
        echo "Downloading zot $VERSION..."
        DOWNLOAD_URL="https://github.com/project-zot/zot/releases/download/${VERSION}/zot-${OS}-${ARCH}-minimal"
        if [ "$OS" = "windows" ]; then
            DOWNLOAD_URL="${DOWNLOAD_URL}.exe"
        fi
        curl -L $DOWNLOAD_URL --output $BINARY
        chmod +x $BINARY
    else
        echo "zot binary already exists, skipping download"
    fi
}

function create_zot_configuration() {
    echo "Creating zot configuration at $CONFIG_FILE..."
    touch $LOG_FILE
    cat > "$CONFIG_FILE" << EOF
{
    "distSpecVersion": "1.0.1",
    "storage": {
        "rootDirectory": "$STORAGE_PATH"
    },
    "http": {
        "address": "0.0.0.0",
        "port": "$PORT"
    },
    "log": {
        "level": "debug",
        "output": "$LOG_FILE"
    }
}
EOF
}

function run() {
    if [[ ! -f $BINARY || ! -f $CONFIG_FILE ]]; then
        echo "zot is not installed. Run '$0 install' first."
        exit 0
    fi

    echo "Starting zot registry on port 0.0.0.0:$PORT..."
    $BINARY serve "$CONFIG_FILE"
}


# ----- MAIN -----
if [[ $OPTION != "help" && $OPTION != "install" && $OPTION != "run" && $OPTION != "uninstall" ]]; then
    echo "Invalid option: $OPTION"
    print_usage
    exit 1
elif [[ $OPTION == "help" || $OPTION == "empty" ]]; then
    print_usage
    exit 0
elif [[ $OPTION == "uninstall" ]]; then
    echo "Uninstalling zot registry..."
    rm -rf ~/.zot
    echo "Uninstalled completed"
    exit 0
fi

if [ "$OS" = "unknown" ]; then
    echo "Error: Unsupported operating system"
    exit 1
fi
echo "Detected OS: $OS"

if [ "$ARCH" = "unknown" ]; then
    echo "Error: Unsupported architecture"
    exit 1
fi
echo "Detected architecture: $ARCH"

if [[ $OPTION == "install" ]]; then
    install
    create_zot_configuration
elif [[ $OPTION == "run" ]]; then
    create_zot_configuration
    run
fi
