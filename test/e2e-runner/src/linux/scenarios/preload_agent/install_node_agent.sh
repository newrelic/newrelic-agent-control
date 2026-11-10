#!/usr/bin/env bash
# Installs the Node.js APM agent from npm into the directory where the preload library looks for
# it: <agent-dir>/node_apm_agent/instrumentation/{node_modules/newrelic,bootstrap.js}. Idempotent.
# Requires npm.
# Usage: install_node_agent.sh <node-agent-version> <agent-dir>
set -euo pipefail

version=${1:?node agent version is required}
agent_dir=${2:?agent dir is required}

instrumentation_dir="${agent_dir}/node_apm_agent/instrumentation"
installed_version_file="${instrumentation_dir}/.installed-version"

if [ -f "${instrumentation_dir}/bootstrap.js" ] && [ "$(cat "${installed_version_file}" 2>/dev/null)" = "${version}" ]; then
  echo "Node agent ${version} already installed at ${instrumentation_dir}"
  exit 0
fi

work_dir=$(mktemp -d)
trap 'rm -rf "${work_dir}"' EXIT

npm install --prefix "${work_dir}" --no-audit --no-fund "newrelic@${version}"
echo "require('newrelic')" > "${work_dir}/bootstrap.js"

rm -rf "${instrumentation_dir}"
mkdir -p "${instrumentation_dir}"
cp -r "${work_dir}/node_modules" "${work_dir}/bootstrap.js" "${instrumentation_dir}/"
chmod -R go+r "${instrumentation_dir}"
echo "${version}" > "${installed_version_file}"
echo "Installed Node agent ${version} at ${instrumentation_dir}"
