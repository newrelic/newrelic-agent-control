#!/usr/bin/env bash

# Helper script to set up a local OCI registry using zot.
# The script is prepared to work in local environments and the CI.
# Supports: Linux, macOS and Windows WSL

# -e: Exit on error
# -u: Exit if undefined variable is used
# -o pipefail: Return failure if any command in a pipeline fails
set -euo pipefail


# ----- CONSTANTS -----
readonly CONFIG_FOLDER="$HOME/.zot"
readonly CONFIG_FILE="$CONFIG_FOLDER/config.json"
readonly BINARY="$CONFIG_FOLDER/zot"
readonly LOG_FILE="$CONFIG_FOLDER/zot.log"


# ----- ARGUMENTS -----
OPTION="${1:-empty}"
VERSION="${VERSION:-v2.1.11}"
PORT="${PORT:-5000}"


# ----- HELPER FUNCTIONS -----
function detect_os() {
    case "$(uname -s)" in
        Linux*)     echo "linux";;
        Darwin*)    echo "darwin";;
        MINGW**)    echo "windows";;
        *)          echo "unknown";;
    esac
}

function detect_arch() {
    case "$(uname -m)" in
        x86_64*)    echo "amd64";;
        amd64*)     echo "amd64";;
        aarch64*)   echo "arm64";;
        arm64*)     echo "arm64";;
        *)          echo "unknown";;
    esac
}

function download_zot() {
    DOWNLOAD_URL="https://github.com/project-zot/zot/releases/download/${VERSION}/zot-${OS}-${ARCH}-minimal"
    if [ "$OS" = "windows" ]; then
        DOWNLOAD_URL="${DOWNLOAD_URL}.exe"
    fi
    curl -L $DOWNLOAD_URL --output $BINARY
    chmod +x $BINARY
}

function create_zot_configuration() {
    touch $LOG_FILE
    
    STORAGE_PATH="$CONFIG_FOLDER/storage"
    LOG_PATH="$LOG_FILE"

    # Make sure paths are compatible with Windows mingw, when using that environment
    if [ "$OS" = "windows" ]; then
        STORAGE_PATH=$(cygpath -m "$STORAGE_PATH")
        LOG_PATH=$(cygpath -m "$LOG_PATH")
    fi
    
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
        "output": "$LOG_PATH"
    }
}
EOF
}

function print_usage() {
  echo "Usage: $0 [run|clean|help]"
  echo "  run   - Run zot (if not present, it will be downloaded and configured)"
  echo "  clean - Remove zot registry and all associated files"
  echo "  help  - Display this help message"
}


# ----- MAIN -----
if [[ $OPTION == "help" || $OPTION == "empty" ]]; then
    print_usage
    exit 0
elif [[ $OPTION != "help" && $OPTION != "run" && $OPTION != "clean" ]]; then
    echo "Invalid option: $OPTION"
    echo "Usage: $0 [run|uninstall|help]"
    exit 1
elif [[ $OPTION == "clean" ]]; then
    echo "Uninstalling zot registry..."
    rm -rf ~/.zot
    echo "Uninstalled completed"
    exit 0
fi


if [[ ! -d ~/.zot ]]; then
    mkdir ~/.zot
fi


OS=$(detect_os)
if [ "$OS" = "unknown" ]; then
    echo "Error: Unsupported operating system"
    exit 1
fi
echo "Detected OS: $OS"


echo "Downloading zot $VERSION..."
if [[ ! -f $CONFIG_FOLDER/zot ]]; then
    ARCH=$(detect_arch)
    if [ "$ARCH" = "unknown" ]; then
        echo "Error: Unsupported architecture"
        exit 1
    fi
    echo "Detected architecture: $ARCH"
    download_zot
else
    echo "zot binary already exists, skipping download"
fi


echo "Creating zot configuration at $CONFIG_FILE..."
create_zot_configuration


echo "Starting zot registry on port 0.0.0.0:$PORT..."
$BINARY serve "$CONFIG_FILE"
