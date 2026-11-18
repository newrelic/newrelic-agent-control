# Scenario of the process scoping proof of concept, run in the test driver of a machine called
# `machine`. EXPECTED_LAYOUT is prepended by process-scoping.nix.
#
# Each backend starts a probe process (it records its own cgroups as its first action) under a
# supervisor that runs as a systemd service with KillMode=mixed, the harshest mode for children
# left in the unit. The supervisor is then killed with SIGKILL, or stopped.
BIN = "/usr/local/bin/scoping-poc"
BACKENDS = ["none", "cgroup", "systemd-run", "dbus-scope", "dbus-service"]
SURVIVES = {"none": False}  # every other backend must leave the child alive


def marker(backend, tag):
    """Where the probe must find itself from its first instruction."""
    return {
        "none": f"poc-sup-{tag}.service",
        "cgroup": f"ac-poc/{tag}",
        "systemd-run": f"ac-poc-{tag}.scope",
        "dbus-scope": f"ac-poc-{tag}.scope",
        "dbus-service": f"ac-poc-{tag}.service",
    }[backend]


def alive(pid):
    return machine.execute(f"kill -0 {pid}")[0] == 0


def probe_pids(tag):
    # Anchored: the supervisor's own command line also contains the probe's arguments.
    out = machine.succeed(f"pgrep -f '^{BIN} probe /run/poc/{tag}.cgroup$' || true")
    return out.split()


def start(backend, tag):
    machine.succeed(
        f"systemd-run --unit=poc-sup-{tag} --collect -p KillMode=mixed "
        f"{BIN} run --backend {backend} --name {tag} -- {BIN} probe /run/poc/{tag}.cgroup"
    )
    machine.wait_until_succeeds(f"test -s /run/poc/{tag}.cgroup", timeout=60)
    [pid] = probe_pids(tag)
    first = machine.succeed(f"cat /run/poc/{tag}.cgroup")
    print(f"[{backend}] probe {pid} started in:\n{first}")
    assert marker(backend, tag) in first, (
        f"the probe did not start inside {marker(backend, tag)}"
    )
    if backend != "none":
        current = machine.succeed(f"cat /proc/{pid}/cgroup")
        unit = f"poc-sup-{tag}.service"
        # Systemd tracks units in the unified hierarchy on v2 and hybrid, and in name=systemd on v1;
        # that is the one that decides what is killed with the unit.
        tracking = next(
            line
            for line in current.splitlines()
            if (
                line.startswith("0::")
                if EXPECTED_LAYOUT in ("v2", "hybrid")
                else ":name=systemd:" in line
            )
        )
        assert unit not in tracking, f"still tracked in the supervisor unit: {tracking}"
        leftovers = [line for line in current.splitlines() if unit in line]
        if leftovers:
            print(f"[{backend}] still accounted to the supervisor unit in: {leftovers}")
    # No thread may outlive a D-Bus call: a connection kept alive between calls would leave a
    # helper thread of zbus behind. A supervisor that polls D-Bus shows one only during a call.
    main = machine.succeed(f"systemctl show -p MainPID --value poc-sup-{tag}").strip()
    counts = [
        int(machine.succeed(f"sleep 0.2; ls /proc/{main}/task | wc -l"))
        for _ in range(10)
    ]
    print(f"[{backend}] supervisor threads over time: {counts}")
    assert min(counts) == 1, f"a thread outlives the D-Bus calls: {counts}"
    return pid


def check_fate(backend, tag, pid):
    if SURVIVES.get(backend, True):
        machine.succeed("sleep 3")
        assert alive(pid), "the child must survive the end of its supervisor"
        # What a restarted supervisor would do to find the child again.
        found = machine.succeed(f"{BIN} pids --backend {backend} --name {tag}").split()
        assert str(pid) in found, f"{pid} not found by the scope, got {found}"
        machine.succeed(f"{BIN} stop --backend {backend} --name {tag}")
        machine.wait_until_fails(f"kill -0 {pid}", timeout=30)
    else:
        machine.wait_until_fails(f"kill -0 {pid}", timeout=30)


def sigkill_supervisor(backend, tag):
    pid = start(backend, tag)
    main = machine.succeed(f"systemctl show -p MainPID --value poc-sup-{tag}").strip()
    machine.succeed(f"kill -9 {main}")
    machine.wait_until_fails(f"systemctl is-active --quiet poc-sup-{tag}", timeout=30)
    check_fate(backend, tag, pid)


def stop_supervisor(backend, tag):
    pid = start(backend, tag)
    machine.succeed(f"systemctl stop poc-sup-{tag}")
    check_fate(backend, tag, pid)


with subtest("the host uses the expected cgroup hierarchy"):
    machine.succeed(f"install -m 0755 /mnt/poc/process-scoping-poc {BIN}")
    machine.succeed("mkdir -p /run/poc")
    layout = machine.succeed(f"{BIN} layout").strip().lower()
    print(
        f"cgroup layout: {layout}, systemd {machine.succeed('systemctl --version | head -1').strip()}"
    )
    assert layout == EXPECTED_LAYOUT, f"expected {EXPECTED_LAYOUT}, found {layout}"

failures = {}
for backend in BACKENDS:
    for label, scenario in [("sigkill", sigkill_supervisor), ("stop", stop_supervisor)]:
        tag = f"{backend}-{label}"
        try:
            with subtest(f"{backend}: supervisor {label}"):
                scenario(backend, tag)
        except Exception as err:
            failures[f"{backend}/{label}"] = str(err)
            print(
                machine.execute(f"journalctl -u 'poc-sup-{tag}*' --no-pager -n 40")[1]
            )
            machine.execute(f"{BIN} stop --backend {backend} --name {tag}")
        machine.execute(f"systemctl reset-failed 'poc-sup-{tag}*'")

print("RESULTS (backend/scenario: ok or reason)")
for backend in BACKENDS:
    for label in ["sigkill", "stop"]:
        key = f"{backend}/{label}"
        print(f"  {key}: {failures.get(key, 'ok')}")
assert not failures, f"{len(failures)} scenarios failed: {sorted(failures)}"
