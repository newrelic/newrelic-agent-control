#!/usr/bin/env bash
# PoC manual validation for the on-host crash-survival CDD/spike (spike-plan item #1).
#
# Not run by CI. This exercises the real systemd unit + real AC binary on a real Linux
# host with systemd, which this repo's own sandbox tooling can't do (no systemd, no
# Windows target here) — see the CDD's "Open questions" for exactly what this settles:
# does `KillMode=process` really leave an orphaned sub-agent alive across a crash, does
# the restarted AC land in the same cgroup, and does it actually adopt instead of
# respawning a duplicate.
#
# Usage: run as root on a systemd-managed Linux host with this branch's
# newrelic-agent-control already built and installed (binary at /usr/bin/newrelic-agent-control,
# unit at /etc/systemd/system/newrelic-agent-control.service or wherever your package
# manager put build/package/newrelic-agent-control.service), and at least one sub-agent
# already configured and running under it. Adjust SUB_AGENT_PATTERN below to match
# whatever that sub-agent's process looks like in `ps`.
#
#   sudo ./scripts/poc-validate-crash-survival.sh

set -euo pipefail

SUB_AGENT_PATTERN="${SUB_AGENT_PATTERN:-sleep}"

if [ "$(id -u)" -ne 0 ]; then
  echo "must run as root" >&2
  exit 1
fi

echo "== reloading systemd unit (in case the KillMode=process change hasn't been picked up) =="
systemctl daemon-reload
systemctl show newrelic-agent-control --property=KillMode

echo
echo "== starting Agent Control =="
systemctl restart newrelic-agent-control
sleep 5

AC_PID="$(systemctl show newrelic-agent-control --property=MainPID --value)"
if [ -z "$AC_PID" ] || [ "$AC_PID" = "0" ]; then
  echo "Agent Control isn't running, nothing to crash. Configure at least one sub-agent first." >&2
  exit 1
fi
echo "Agent Control main PID: $AC_PID"

SUB_AGENT_PID="$(pgrep -f "$SUB_AGENT_PATTERN" | head -n1 || true)"
if [ -z "$SUB_AGENT_PID" ]; then
  echo "no process matching pattern '$SUB_AGENT_PATTERN' found; set SUB_AGENT_PATTERN to something that matches your configured sub-agent" >&2
  exit 1
fi
echo "sub-agent PID (pattern '$SUB_AGENT_PATTERN'): $SUB_AGENT_PID"

echo
echo "== simulating a crash: kill -9 the Agent Control main PID =="
kill -9 "$AC_PID"

echo "waiting for RestartSec + the sub-agent's own liveness to settle..."
sleep 8

echo
echo "== checking the sub-agent survived, unsupervised, during the restart window =="
if kill -0 "$SUB_AGENT_PID" 2>/dev/null; then
  echo "PASS: sub-agent PID $SUB_AGENT_PID is still alive after Agent Control's main PID was killed."
else
  echo "FAIL: sub-agent PID $SUB_AGENT_PID is gone. Check KillMode is actually 'process' (see 'systemctl show' output above) and that the unit was reloaded."
  exit 1
fi

echo
echo "== checking Agent Control actually restarted =="
NEW_AC_PID="$(systemctl show newrelic-agent-control --property=MainPID --value)"
echo "new Agent Control main PID: $NEW_AC_PID"
if [ "$NEW_AC_PID" = "$AC_PID" ] || [ -z "$NEW_AC_PID" ] || [ "$NEW_AC_PID" = "0" ]; then
  echo "FAIL: Agent Control does not appear to have restarted with a new PID." >&2
  exit 1
fi

echo
echo "== checking the new instance landed in the same cgroup as the old one =="
NEW_CGROUP="$(systemctl show newrelic-agent-control --property=ControlGroup --value)"
echo "cgroup: $NEW_CGROUP"
echo "(compare against the cgroup path logged by the previous instance, e.g. via 'journalctl -u newrelic-agent-control' around the time of the kill -9 above — they should match)"

echo
echo "== checking whether Agent Control logged an adoption instead of a respawn =="
echo "looking for the PoC's own log line (see command_os.rs: \"adopted a still-running process\")..."
journalctl -u newrelic-agent-control --since "1 minute ago" | grep -i "adopted a still-running process" \
  && echo "PASS: found an adoption log line." \
  || echo "No adoption log line found — on a first run there may be no bookkeeping yet for this sub-agent (nothing to adopt), or bookkeeping is disabled/misconfigured. Re-run this script a second time against an already-once-adopted host to be sure."

echo
echo "== confirming no duplicate sub-agent process was spawned =="
DUPLICATE_COUNT="$(pgrep -fc "$SUB_AGENT_PATTERN" || true)"
echo "processes matching '$SUB_AGENT_PATTERN': $DUPLICATE_COUNT"
if [ "$DUPLICATE_COUNT" -gt 1 ]; then
  echo "FAIL: more than one matching process is running; Agent Control may have respawned a duplicate instead of adopting." >&2
  exit 1
fi

echo
echo "All checks passed."
