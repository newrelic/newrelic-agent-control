#!/usr/bin/env bash
# Unit test for preremove.sh: on a true removal ($1=0 for RPM, $1=remove for DEB) it must
# stop/disable the service; on an upgrade ($1=1/2 for RPM, $1=upgrade for DEB) it must not.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
preremove="$script_dir/preremove.sh"

fake_bin="$(mktemp -d)"
log_file="$(mktemp)"
trap 'rm -rf "$fake_bin" "$log_file"' EXIT

cat > "$fake_bin/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "systemctl $*" >> "$SYSTEMCTL_LOG"
EOF
chmod +x "$fake_bin/systemctl"

export PATH="$fake_bin:$PATH"
export SYSTEMCTL_LOG="$log_file"

fail=0

assert_disables() {
  : > "$log_file"
  sh "$preremove" "$1"
  grep -q "disable" "$log_file" || { echo "FAIL: expected stop/disable for \$1=$1"; fail=1; }
}

assert_skips() {
  : > "$log_file"
  sh "$preremove" "$1"
  [ -s "$log_file" ] && { echo "FAIL: unexpected systemctl call for \$1=$1: $(cat "$log_file")"; fail=1; }
  return 0
}

# True removal: RPM %preun passes 0, DEB prerm passes "remove".
assert_disables "0"
assert_disables "remove"

# Upgrade in progress: RPM %preun passes >=1, DEB prerm passes "upgrade".
assert_skips "1"
assert_skips "2"
assert_skips "upgrade"

if [ "$fail" -eq 1 ]; then
  echo "preremove.sh test FAILED"
  exit 1
fi
echo "preremove.sh test passed"
