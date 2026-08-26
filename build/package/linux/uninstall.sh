#!/bin/bash
set -e

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root." >&2
    exit 1
fi

detect_package_manager() {
    if command -v apt-get >/dev/null 2>&1; then
        echo "apt"
    elif command -v yum >/dev/null 2>&1; then
        echo "yum"
    elif command -v zypper >/dev/null 2>&1; then
        echo "zypper"
    else
        echo "No supported package manager found (apt-get, yum, zypper)." >&2
        exit 1
    fi
}

remove_agent_control_package() {
    case "$1" in
        apt)
            OPTIONS="-o DPkg::Lock::Timeout=60"
            if [ -n "$HTTPS_PROXY" ]; then
                OPTIONS="$OPTIONS -o Acquire::Http::Proxy=$HTTPS_PROXY"
            fi

            apt-get $OPTIONS remove -y -qq newrelic-agent-control || true
            ;;
        yum)
            yum -y -q remove newrelic-agent-control || true
            ;;
        zypper)
            zypper -n --quiet remove newrelic-agent-control || true
            ;;
    esac
}


PKG_MANAGER="$(detect_package_manager)"

remove_agent_control_package "$PKG_MANAGER"
echo "New Relic Agent Control has been removed from this host."
rm -- "$0" 2>/dev/null || true
