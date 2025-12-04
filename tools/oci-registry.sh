#!/usr/bin/env bash

# Helper script to set up a local OCI registry using zot.
# The script is prepared to work in local environments and the CI.
# Supports: Linux, macOS and Windows mingw

# -e: Exit on error
# -u: Exit if undefined variable is used
# -o pipefail: Return failure if any command in a pipeline fails
set -euo pipefail


# ----- CONSTANTS -----
UNAME_S=$(uname -s)
case "$UNAME_S" in
    Linux*)     OS="linux";;
    Darwin*)    OS="darwin";;
    MINGW*)     OS="windows";;
    *)          OS="unknown";;
esac
readonly OS

UNAME_M=$(uname -m)
case "$UNAME_M" in
    x86_64*)    ARCH="amd64";;
    amd64*)     ARCH="amd64";;
    aarch64*)   ARCH="arm64";;
    arm64*)     ARCH="arm64";;
    *)          ARCH="unknown";;
esac
readonly ARCH

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
  echo "Usage: $0 [install|run|uninstall|killall|help]"
  echo "  install    - Download and install zot registry"
  echo "  run        - Run zot registry"
  echo "  uninstall  - Remove zot registry and all associated files"
  echo "  killall    - Kill all running zot processes"
  echo "  help       - Display this help message"
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
if [[ $OPTION != "help" && $OPTION != "install" && $OPTION != "uninstall" && $OPTION != "run" && $OPTION != "killall" ]]; then
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
elif [[ $OPTION == "killall" ]]; then
    echo "Killing all zot processes..."
    if [ "$OS" = "windows" ]; then
        taskkill //F //IM zot* || echo "No zot processes found"
    else
        pkill -f zot || echo "No zot processes found"
    fi
    echo "Killed all zot processes"
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
