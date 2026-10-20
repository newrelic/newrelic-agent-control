#!/usr/bin/env bash
# Entrypoint of the preload agent post_download_hook (runs on every install).
# Usage: install.sh <java-agent-version> <java-agent-parent-dir>
# PACKAGE_DIR is provided by Agent Control.
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

bash "${script_dir}/install_preload.sh"
bash "${script_dir}/install_java_agent.sh" "$@"
