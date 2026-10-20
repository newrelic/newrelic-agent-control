#!/usr/bin/env bash
# Downloads the Java APM agent from Maven Central into the directory where the preload library
# looks for it: <agent-dir>/java_apm_agent/newrelic.{jar,yml}. Idempotent.
# Usage: install_java_agent.sh <java-agent-version> <agent-dir>
set -euo pipefail

version=${1:?java agent version is required}
agent_dir=${2:?agent dir is required}

dest="${agent_dir}/java_apm_agent"
installed_version_file="${dest}/.installed-version"

if [ -f "${dest}/newrelic.jar" ] && [ "$(cat "${installed_version_file}" 2>/dev/null)" = "${version}" ]; then
  echo "Java agent ${version} already installed at ${dest}"
  exit 0
fi

base_url="https://repo1.maven.org/maven2/com/newrelic/agent/java/newrelic-java/${version}"
zip_name="newrelic-java-${version}.zip"

work_dir=$(mktemp -d)
trap 'rm -rf "${work_dir}"' EXIT

curl -fsSL --retry 3 -o "${work_dir}/${zip_name}" "${base_url}/${zip_name}"

expected_sha1=$(curl -fsSL --retry 3 "${base_url}/${zip_name}.sha1")
actual_sha1=$(sha1sum "${work_dir}/${zip_name}" | cut -d ' ' -f 1)
if [ "${expected_sha1}" != "${actual_sha1}" ]; then
  echo "Checksum mismatch for ${zip_name}: expected ${expected_sha1}, got ${actual_sha1}" >&2
  exit 1
fi

unzip -q "${work_dir}/${zip_name}" -d "${work_dir}/extracted"

mkdir -p "${dest}"
install -m 0644 "${work_dir}/extracted/newrelic/newrelic.jar" "${dest}/newrelic.jar"
install -m 0644 "${work_dir}/extracted/newrelic/newrelic.yml" "${dest}/newrelic.yml"
echo "${version}" > "${installed_version_file}"
echo "Installed Java agent ${version} at ${dest}"
