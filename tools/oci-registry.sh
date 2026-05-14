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

# FOR LOCAL TEST ENVIRONMENTS ONLY
readonly LOCAL_TEST_ONLY_USERNAME="fake-user"
readonly LOCAL_TEST_ONLY_PASSWORD="fake-password"
# Pre-computed regenerate with: htpasswd -nbB $LOCAL_TEST_ONLY_USERNAME $LOCAL_TEST_ONLY_PASSWORD
readonly LOCAL_TEST_ONLY_HTPASSWD_ENTRY='fake-user:$2y$05$WLvLO1ojdi2NtziBhbb5ge8fQK.aNz2sjCwQ.aS7WpZo1ujmmnIQW'

if [ "$OS" = "windows" ]; then
    STORAGE_PATH=$(cygpath -m "$CONFIG_FOLDER/storage")
    readonly STORAGE_PATH
    LOG_FILE=$(cygpath -m "$CONFIG_FOLDER/zot.log")
    readonly LOG_FILE
    HTPASSWD_FILE=$(cygpath -m "$CONFIG_FOLDER/htpasswd")
    readonly HTPASSWD_FILE
else
    readonly STORAGE_PATH="$CONFIG_FOLDER/storage"
    readonly LOG_FILE="$CONFIG_FOLDER/zot.log"
    readonly HTPASSWD_FILE="$CONFIG_FOLDER/htpasswd"
fi


# ----- HELPER FUNCTIONS -----
function print_usage() {
  echo "Usage: $0 [install|run|uninstall|killall|help] [--basic-auth]"
  echo "  install    - Download and install zot registry"
  echo "  run        - Run zot registry"
  echo "  uninstall  - Remove zot registry and all associated files"
  echo "  killall    - Kill all running zot processes"
  echo "  help       - Display this help message"
  echo ""
  echo "Flags:"
  echo "  --basic-auth  Enable basic authentication with fake credentials ($LOCAL_TEST_ONLY_USERNAME:$LOCAL_TEST_ONLY_PASSWORD)."
}


# ----- ARGUMENTS -----
OPTION="${1:-empty}"
VERSION="${VERSION:-v2.1.11}"
PORT="${PORT:-5001}"
BASIC_AUTH="${BASIC_AUTH:-false}"

# Parse flags from remaining arguments
shift || true
for arg in "$@"; do
    case "$arg" in
        --basic-auth) BASIC_AUTH="true";;
        *) echo "Unknown flag: $arg"; print_usage; exit 1;;
    esac
done

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
        curl -L "$DOWNLOAD_URL" --output "$BINARY"
        chmod +x "$BINARY"
    else
        echo "zot binary already exists, skipping download"
    fi
}

function create_htpasswd_file() {
    echo "Creating htpasswd file for local-test-only credentials..."
    echo "$LOCAL_TEST_ONLY_HTPASSWD_ENTRY" > "$HTPASSWD_FILE"
}

function create_zot_configuration() {
    echo "Creating zot configuration at $CONFIG_FILE..."
    touch "$LOG_FILE"

    local http_auth_block=""
    if [ "$BASIC_AUTH" = "true" ]; then
        http_auth_block=$(cat << EOF
,
        "auth": {
            "htpasswd": {
                "path": "$HTPASSWD_FILE"
            }
        }
EOF
)
    fi

    # TLS_CERT and TLS_KEY are paths to a PEM cert and its matching key; both must be set to enable HTTPS.
    local http_tls_block=""
    if [ -n "${TLS_CERT:-}" ] && [ -n "${TLS_KEY:-}" ]; then
        http_tls_block=$(cat << EOF
,
        "tls": {
            "cert": "$TLS_CERT",
            "key": "$TLS_KEY"
        }
EOF
)
    fi

    cat > "$CONFIG_FILE" << EOF
{
    "distSpecVersion": "1.0.1",
    "storage": {
        "rootDirectory": "$STORAGE_PATH"
    },
    "http": {
        "address": "0.0.0.0",
        "port": "$PORT"${http_auth_block}${http_tls_block}
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
    if [ "$BASIC_AUTH" = "true" ]; then
        create_htpasswd_file
    fi
    create_zot_configuration
elif [[ $OPTION == "run" ]]; then
    if [ "$BASIC_AUTH" = "true" ]; then
        create_htpasswd_file
    fi
    create_zot_configuration
    run
fi
