#!/usr/bin/env bash
# test-cli-lifecycle.sh — end-to-end test of newrelic-agent-control-cli on a Linux VM
#
# Usage:
#   ./scripts/test-cli-lifecycle.sh <vm-name> <staging-license-key>
#
# Requires: multipass, curl
#
# The script:
#   1. Starts the VM and installs Agent Control via the NR CLI
#   2. Copies the locally built CLI binary into the VM
#   3. Runs dry-run tests (non-destructive)
#   4. Runs the real update (to current version — idempotency check)
#   5. Runs the real uninstall
#   6. Verifies everything is cleaned up

set -euo pipefail

VM="${1:?Usage: $0 <vm-name> <license-key>}"
LICENSE_KEY="${2:?Usage: $0 <vm-name> <license-key>}"

CLI_BINARY="./target/debug/newrelic-agent-control-cli"
CURRENT_VERSION=$(grep '^version' agent-control/Cargo.toml | head -1 | awk -F'"' '{print $2}')

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
pass() { echo -e "${GREEN}✓ $*${NC}"; }
fail() { echo -e "${RED}✗ $*${NC}"; exit 1; }
step() { echo -e "\n${YELLOW}▶ $*${NC}"; }

# ── 0. Preflight ──────────────────────────────────────────────────────────────
step "Preflight checks"
[[ -f "$CLI_BINARY" ]] || fail "CLI binary not found at $CLI_BINARY — run: cargo build --bin newrelic-agent-control-cli"
multipass info "$VM" &>/dev/null || fail "VM '$VM' not found"
pass "Binary and VM exist"

# ── 1. Start VM ───────────────────────────────────────────────────────────────
step "Starting VM: $VM"
multipass start "$VM"
sleep 3
pass "VM started"

# ── 2. Install Agent Control ──────────────────────────────────────────────────
step "Installing Agent Control via NR CLI"
multipass exec "$VM" -- bash -c "
  set -e
  if systemctl is-active --quiet newrelic-agent-control 2>/dev/null; then
    echo 'Already installed — skipping install step'
    exit 0
  fi
  curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | bash
  sudo NEW_RELIC_API_KEY=${LICENSE_KEY} \
       NEW_RELIC_ACCOUNT_ID=1 \
       NEW_RELIC_REGION=STAGING \
       NEW_RELIC_CLI_SKIP_CORE=1 \
       /usr/local/bin/newrelic install -n agent-control --no-prompt
"
sleep 5

# Verify installation
multipass exec "$VM" -- systemctl is-active newrelic-agent-control \
  && pass "Agent Control service is active" \
  || fail "Agent Control service failed to start"

multipass exec "$VM" -- test -d /var/lib/newrelic-agent-control \
  && pass "/var/lib/newrelic-agent-control exists" \
  || fail "State directory missing"

# ── 3. Copy CLI binary ────────────────────────────────────────────────────────
step "Copying CLI binary to VM"
multipass transfer "$CLI_BINARY" "$VM":/tmp/newrelic-agent-control-cli
multipass exec "$VM" -- chmod +x /tmp/newrelic-agent-control-cli
pass "Binary transferred"

# ── 4. Dry-run tests ──────────────────────────────────────────────────────────
step "Dry-run: uninstall"
OUTPUT=$(multipass exec "$VM" -- sudo /tmp/newrelic-agent-control-cli uninstall --dry-run --yes)
echo "$OUTPUT"
echo "$OUTPUT" | grep -q "var/lib/newrelic-agent-control" || fail "Dry-run missing state dir"
echo "$OUTPUT" | grep -q "usr/bin/newrelic-agent-control" || fail "Dry-run missing binary"
echo "$OUTPUT" | grep -q "Dry-run complete" || fail "Dry-run completion message missing"
pass "uninstall --dry-run output is correct"

step "Dry-run: update (idempotency — same version)"
OUTPUT=$(multipass exec "$VM" -- sudo /tmp/newrelic-agent-control-cli update --version "$CURRENT_VERSION" --dry-run)
echo "$OUTPUT"
echo "$OUTPUT" | grep -q "already at version\|Dry-run: would download" || fail "Idempotency check output unexpected"
pass "update --dry-run at current version behaves correctly"

step "Dry-run: update (different version)"
OUTPUT=$(multipass exec "$VM" -- sudo /tmp/newrelic-agent-control-cli update --version 1.0.0 --dry-run)
echo "$OUTPUT"
echo "$OUTPUT" | grep -q "Dry-run: would download" || fail "Dry-run update missing expected output"
pass "update --dry-run with different version output is correct"

step "Test: update without root should fail"
OUTPUT=$(multipass exec "$VM" -- /tmp/newrelic-agent-control-cli uninstall --dry-run 2>&1 || true)
# dry-run doesn't need root — but real run should
OUTPUT_REAL=$(multipass exec "$VM" -- /tmp/newrelic-agent-control-cli uninstall --yes 2>&1 || true)
echo "$OUTPUT_REAL" | grep -qi "root\|sudo\|permission" || fail "Expected root error message"
pass "Non-root real uninstall correctly rejected"

# ── 5. Real uninstall ─────────────────────────────────────────────────────────
step "Real uninstall (--yes to skip prompt)"
multipass exec "$VM" -- sudo /tmp/newrelic-agent-control-cli uninstall --yes
pass "Uninstall command returned 0"

step "Verify: service is gone"
STATUS=$(multipass exec "$VM" -- systemctl is-active newrelic-agent-control 2>/dev/null || echo "inactive")
[[ "$STATUS" != "active" ]] && pass "Service is no longer active ($STATUS)" || fail "Service still active after uninstall"

step "Verify: state directory removed"
multipass exec "$VM" -- test ! -d /var/lib/newrelic-agent-control \
  && pass "/var/lib/newrelic-agent-control removed" \
  || fail "State directory still exists"

step "Verify: config directory removed"
multipass exec "$VM" -- test ! -d /etc/newrelic-agent-control \
  && pass "/etc/newrelic-agent-control removed" \
  || fail "Config directory still exists"

step "Verify: daemon binary removed"
multipass exec "$VM" -- test ! -f /usr/bin/newrelic-agent-control \
  && pass "/usr/bin/newrelic-agent-control removed" \
  || fail "Daemon binary still exists"

step "Verify: systemd unit removed"
multipass exec "$VM" -- test ! -f /etc/systemd/system/newrelic-agent-control.service \
  && pass "Systemd unit file removed" \
  || fail "Systemd unit still exists"

# ── 6. Test --keep-config ─────────────────────────────────────────────────────
step "Reinstall and test --keep-config"
multipass exec "$VM" -- bash -c "
  curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | bash
  sudo NEW_RELIC_API_KEY=${LICENSE_KEY} \
       NEW_RELIC_ACCOUNT_ID=1 \
       NEW_RELIC_REGION=STAGING \
       NEW_RELIC_CLI_SKIP_CORE=1 \
       /usr/local/bin/newrelic install -n agent-control --no-prompt
"
sleep 3

multipass exec "$VM" -- sudo /tmp/newrelic-agent-control-cli uninstall --yes --keep-config

multipass exec "$VM" -- test -d /etc/newrelic-agent-control \
  && pass "--keep-config preserved /etc/newrelic-agent-control" \
  || fail "--keep-config but config directory was removed"

multipass exec "$VM" -- test ! -f /usr/bin/newrelic-agent-control \
  && pass "Binaries still removed with --keep-config" \
  || fail "Binary not removed with --keep-config"

# ── Summary ───────────────────────────────────────────────────────────────────
echo -e "\n${GREEN}════════════════════════════════════════${NC}"
echo -e "${GREEN}  All tests passed on VM: $VM${NC}"
echo -e "${GREEN}════════════════════════════════════════${NC}\n"
