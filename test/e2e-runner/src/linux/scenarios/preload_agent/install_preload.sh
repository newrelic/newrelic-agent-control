#!/usr/bin/env bash
# Stages the preload library extracted by Agent Control and enables it host-wide.
# Idempotent. Expects PACKAGE_DIR from Agent Control.
set -euo pipefail

# World-readable location: the Agent Control data dir is not traversable by non-root users, and a
# library that cannot be opened by a process breaks it when listed in /etc/ld.so.preload.
staged_so=/usr/local/lib/newrelic-preload-agent.so

so_path=$(find "${PACKAGE_DIR}" -name '*.so' -print -quit)
if [ -z "${so_path}" ]; then
  echo "No .so file found in extracted preload package at ${PACKAGE_DIR}" >&2
  exit 1
fi

install -m 0644 -o root -g root "${so_path}" "${staged_so}"
grep -qxF "${staged_so}" /etc/ld.so.preload 2>/dev/null || echo "${staged_so}" >> /etc/ld.so.preload
