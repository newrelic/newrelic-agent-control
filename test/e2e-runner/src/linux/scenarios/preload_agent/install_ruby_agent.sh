#!/usr/bin/env bash
# Installs the Ruby APM agent gem from rubygems.org into the directory where the preload library
# looks for it: <agent-dir>/ruby_apm_agent/instrumentation/newrelic_rpm-<version>. Idempotent.
# Requires ruby (and its bundled gem command).
# Usage: install_ruby_agent.sh <ruby-agent-version> <agent-dir>
set -euo pipefail

version=${1:?ruby agent version is required}
agent_dir=${2:?agent dir is required}

instrumentation_dir="${agent_dir}/ruby_apm_agent/instrumentation"
gem_dir="${instrumentation_dir}/newrelic_rpm-${version}"

if [ -f "${gem_dir}/lib/boot/strap.rb" ]; then
  echo "Ruby agent ${version} already installed at ${gem_dir}"
  exit 0
fi

work_dir=$(mktemp -d)
trap 'rm -rf "${work_dir}"' EXIT

gem install newrelic_rpm --version "${version}" --no-document --install-dir "${work_dir}"

# Only the agent gem is kept, as the preload library expects.
rm -rf "${instrumentation_dir}"
mkdir -p "${instrumentation_dir}"
cp -r "${work_dir}/gems/newrelic_rpm-${version}" "${gem_dir}"
chmod -R go+r "${instrumentation_dir}"
echo "Installed Ruby agent ${version} at ${gem_dir}"
