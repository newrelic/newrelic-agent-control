#!/usr/bin/env bash
# Installs the Python APM agent from PyPI into the directory where the preload library looks for
# it: <agent-dir>/python_apm_agent/instrumentation/newrelic. Idempotent.
# Requires python3 with pip.
# Usage: install_python_agent.sh <python-agent-version> <agent-dir>
set -euo pipefail

version=${1:?python agent version is required}
agent_dir=${2:?agent dir is required}

instrumentation_dir="${agent_dir}/python_apm_agent/instrumentation"
installed_version_file="${instrumentation_dir}/.installed-version"

if [ -d "${instrumentation_dir}/newrelic" ] && [ "$(cat "${installed_version_file}" 2>/dev/null)" = "${version}" ]; then
  echo "Python agent ${version} already installed at ${instrumentation_dir}"
  exit 0
fi

work_dir=$(mktemp -d)
trap 'rm -rf "${work_dir}"' EXIT

python3 -m pip install --no-input --disable-pip-version-check --break-system-packages \
  --target "${work_dir}" "newrelic==${version}"

rm -rf "${instrumentation_dir}"
mkdir -p "${instrumentation_dir}"
cp -r "${work_dir}/." "${instrumentation_dir}/"
chmod -R go+r "${instrumentation_dir}"
echo "${version}" > "${installed_version_file}"
echo "Installed Python agent ${version} at ${instrumentation_dir}"
