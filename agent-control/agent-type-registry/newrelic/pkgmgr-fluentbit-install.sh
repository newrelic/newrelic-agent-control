#!/usr/bin/env bash
set -euo pipefail

# pkgmgr.fluentbit POC install/upgrade script.
#
# Deferred, not handled by this script (see design doc):
#   - proxy
#   - legacy td-agent-bit (FB 1.9) present without fluent-bit
#   - fluent-bit already installed from a non-NR repo (distro default or fluentbit.io)
#   - package-manager lock contention (concurrent apt/yum/zypper runs)
#   - manually-installed/untracked fluent-bit binaries (not registered with dpkg/rpm)

: "${FLUENT_BIT_VERSION:?FLUENT_BIT_VERSION must be set}"

REPO_BASE="https://download.newrelic.com/infrastructure_agent"
GPG_KEY_URL="${REPO_BASE}/gpg/newrelic-infra.gpg"

log() { echo "[pkgmgr.fluentbit] $*" >&2; }

detect_os_family() {
  . /etc/os-release
  case "${ID:-}" in
    ubuntu|debian) echo apt; return 0 ;;
    rhel|centos|fedora|amzn|rocky|almalinux) echo yum; return 0 ;;
    sles|opensuse-leap|opensuse-tumbleweed) echo zypper; return 0 ;;
  esac
  case " ${ID_LIKE:-} " in
    *debian*) echo apt; return 0 ;;
    *rhel*|*fedora*) echo yum; return 0 ;;
    *suse*) echo zypper; return 0 ;;
  esac
  log "unsupported OS: ID=${ID:-unknown} ID_LIKE=${ID_LIKE:-unknown}"
  return 1
}

installed_version() {
  case "$1" in
    apt)
      local status
      status="$(dpkg-query -W -f='${Status}' fluent-bit 2>/dev/null || true)"
      [ "$status" = "install ok installed" ] && dpkg-query -W -f='${Version}' fluent-bit 2>/dev/null || true
      ;;
    yum|zypper) rpm -q --qf '%{VERSION}' fluent-bit 2>/dev/null || true ;;
  esac
}

ensure_nr_repo_apt() {
  local list_file="/etc/apt/sources.list.d/newrelic-infra.list"
  local keyring="/usr/share/keyrings/newrelic-infra.gpg"
  local codename
  codename="$(. /etc/os-release && echo "${VERSION_CODENAME:-}")"
  local repo_line="deb [signed-by=${keyring}] ${REPO_BASE}/linux/apt ${codename} main"

  if [ -f "$list_file" ] && grep -qxF "$repo_line" "$list_file"; then
    return 0
  fi

  log "Configuring New Relic apt repository"
  curl -fsSL "$GPG_KEY_URL" | gpg --batch --yes --dearmor -o "$keyring"
  echo "$repo_line" > "$list_file"
  DEBIAN_FRONTEND=noninteractive apt-get update -y
}

install_or_upgrade_apt() {
  local current="$1"
  if [ -z "$current" ]; then
    apt-get install -y "fluent-bit=${FLUENT_BIT_VERSION}"
  else
    apt-get install -y --only-upgrade "fluent-bit=${FLUENT_BIT_VERSION}"
  fi
}

main() {
  local family current
  family="$(detect_os_family)"
  current="$(installed_version "$family")"

  if [ "$current" = "$FLUENT_BIT_VERSION" ]; then
    log "fluent-bit ${FLUENT_BIT_VERSION} already installed, nothing to do"
    return 0
  fi

  case "$family" in
    apt)
      ensure_nr_repo_apt
      install_or_upgrade_apt "$current"
      ;;
    yum)
      echo "TODO"
      ;;
    zypper)
      echo "TODO"
      ;;
  esac

  log "fluent-bit ${FLUENT_BIT_VERSION} install/upgrade complete (was: ${current:-none})"
}

main
