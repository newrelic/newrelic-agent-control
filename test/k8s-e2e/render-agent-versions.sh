#!/usr/bin/env bash
# Replaces the ${<AGENT>_VERSION} and ${<AGENT>_AGENT_TYPE_VERSION} placeholders of the ac-values files in place,
# using the env vars exported by the `agent-versions` action. Meant for CI: it modifies tracked files.
set -euo pipefail

cd "$(dirname "$0")"

for values_file in */ac-values-*.yml; do
  placeholders=$(grep -oE '\$\{[A-Z0-9_]+_VERSION\}' "$values_file" | sort -u || true)
  [ -z "$placeholders" ] && continue

  for placeholder in $placeholders; do
    variable=${placeholder#\$\{}
    variable=${variable%\}}
    if [ -z "${!variable:-}" ]; then
      echo "::error::$variable is not set but $values_file uses it; Instrumentation Metadata has no version for it on kubernetes"
      exit 1
    fi
  done

  envsubst "$placeholders" <"$values_file" >"$values_file.rendered"
  mv "$values_file.rendered" "$values_file"
done
