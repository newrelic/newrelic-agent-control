# Scenario run against the Agent Control systemd service, in the test driver of a machine
# called `machine`. The constants it reads (KILL_MODE, EXPECTED_LAYOUT, ADOPTION, SLEEP_SECONDS)
# are prepended by scenario.nix.
SLEEPER = f"bin/[s]leep {SLEEP_SECONDS}$"
UNIT = "newrelic-agent-control.service"


def sleepers():
    return machine.succeed(f"pgrep -f '{SLEEPER}' || true").split()


def main_pid():
    return machine.succeed(f"systemctl show -p MainPID --value {UNIT}").strip()


def cgroup_layout():
    if machine.succeed("stat -fc %T /sys/fs/cgroup").strip() == "cgroup2fs":
        return "v2"
    if (
        machine.execute('test "$(stat -fc %T /sys/fs/cgroup/unified)" = cgroup2fs')[0]
        == 0
    ):
        return "hybrid"
    return "v1"


def wait_for_sleeper():
    try:
        machine.wait_until_succeeds(f"pgrep -f '{SLEEPER}'", timeout=120)
    except Exception:
        print(machine.execute(f"journalctl -u {UNIT} --no-pager -n 100")[1])
        raise


def wait_for_new_main(old):
    machine.wait_until_succeeds(
        f"systemctl is-active --quiet {UNIT} && "
        f'test "$(systemctl show -p MainPID --value {UNIT})" != 0 && '
        f'test "$(systemctl show -p MainPID --value {UNIT})" != {old}'
    )


def alive(pid):
    return machine.execute(f"kill -0 {pid}")[0] == 0


machine.wait_for_unit(UNIT)

with subtest("the service uses the expected KillMode"):
    assert (
        machine.succeed(f"systemctl show -p KillMode --value {UNIT}").strip()
        == KILL_MODE
    )

with subtest("the host uses the expected cgroup hierarchy"):
    layout = cgroup_layout()
    print(f"cgroup layout: {layout}")
    assert layout == EXPECTED_LAYOUT, f"expected {EXPECTED_LAYOUT}, found {layout}"

with subtest("the sub-agent runs inside the service cgroup"):
    wait_for_sleeper()
    [first_sub] = sleepers()
    machine.succeed(f"grep -q {UNIT} /proc/{first_sub}/cgroup")
    first_main = main_pid()

with subtest("sub-agent fate after killing Agent Control with SIGKILL"):
    machine.succeed(f"kill -9 {first_main}")
    if KILL_MODE == "mixed":
        machine.wait_until_fails(f"kill -0 {first_sub}")
    else:
        assert alive(first_sub), "the sub-agent must survive an Agent Control crash"
    wait_for_new_main(first_main)
    wait_for_sleeper()
    machine.wait_until_succeeds(
        f"test \"$(journalctl -u {UNIT} --no-pager | grep -c 'Starting executable')\" -ge 2",
        timeout=120,
    )
    print(f"sub-agents after restart: {sleepers()}")

with subtest("restarted Agent Control and sub-agents"):
    if KILL_MODE == "mixed":
        assert sleepers() != [first_sub] and len(sleepers()) == 1
    elif ADOPTION:
        assert sleepers() == [first_sub], "the surviving sub-agent must be adopted"
    else:
        assert alive(first_sub)

with subtest("stopping the service and cleaning up leftovers"):
    machine.succeed(f"systemctl stop {UNIT}")
    if KILL_MODE == "process" and not ADOPTION:
        assert alive(first_sub), "KillMode=process leaves the orphan behind on stop"
    machine.execute(f"systemctl kill --kill-who=all {UNIT}")
    machine.wait_until_succeeds(f"! pgrep -f '{SLEEPER}'")
