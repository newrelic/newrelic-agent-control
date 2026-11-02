# poc-linux-crash-survival

Standalone PoC to check whether, on Linux, a sub-agent can **survive an Agent Control (AC) crash** and be
**re-adopted by the restarted AC**, without elevation (except where noted).

It is not wired into the AC binaries. It only lives in the workspace to reuse the pinned `nix` crate. The design and the
comparison of the options are in [`docs/poc-onhost-crash-survival-linux.md`](../docs/poc-onhost-crash-survival-linux.md).

## The idea

Survival itself is free. With `KillMode=process` in the systemd unit (or when AC is simply `kill -9`'d outside systemd) the
sub-agents keep running and are reparented to init. What is hard is **finding them again** and being sure they are the
right ones. Each example implements one way of doing that, end to end:

1. A "supervisor" (the example re-executing itself) starts the sub-agent the way AC would: as the leader of its own process
   group and with no stdio shared with AC.
2. The supervisor leaves whatever record the strategy needs and `abort()`s, which simulates an AC crash.
3. The parent plays the restarted AC: it finds the sub-agent again using **only** what the strategy leaves behind, adopts or
   inspects it and kills it, asserting every step. A failed check exits with a non-zero status.

## What each example probes

| Example | Strategy | Question it answers |
|---|---|---|
| `crash_survival_bookkeeping` | `(pid, start_time)` persisted at spawn | Does the child survive the crash, can it be adopted from the stored values and killed without root? Is a wrong `start_time` (pid reuse) rejected? Does the orphan keep the dead supervisor as parent? |
| `crash_survival_env_tag` | `NR_AC_AGENT_ID` / `NR_AC_EXEC_ID` / `NR_AC_INSTANCE` in the environment, whole-host `/proc` scan | Can the orphan be found with no stored state? Is the group leader picked over a forked worker with the same cmdline and env? Is another AC instance ignored? **Caveats:** a sub-agent that scrubs its environment (`env -i`) is *not* found; workers whose leader died are reported as orphans and killed. |
| `crash_survival_cgroup` | A named cgroup per sub-agent is the only record | Can the restarted AC find the members by name, with the sub-agent having scrubbed its environment, and kill them atomically (`cgroup.kill`)? Needs cgroup v2 and write access to AC's cgroup. |
| `crash_survival_shim` | A shim process is the real parent and AC reconnects to its unix socket | Can AC reconnect with no scanning, read the **real exit status** of a sub-agent that died while AC was down, and keep its output (written to a file the shim owns)? |

Library pieces (`src/`):

- `lib.rs`: `/proc/<pid>/stat` parsing (`read_stat`), `AdoptedProcess` (adopt with a `start_time` check, wait, kill),
  `spawn_detached` (own process group, no stdio), `wait_until`.
- `tag.rs`: `Identity` (markers + exact cmdline), `scan` (whole-host `/proc` scan, `cmdline` first), `select_root` (group
  leader, not a worker), `kill_orphans` (never signals AC's own group or init's).
- `cgroup.rs`: `own_cgroup_dir`, `Cgroup` (create/open by name, `procs`, `live_members`, `kill_all`, `remove`) and
  `spawn_in_cgroup` (the child moves itself into the cgroup between `fork` and `exec`, using syscalls only).
- `shim.rs`: the shim loop (`run`: subreaper, child output to a log file, unix socket) and the client side (`query`,
  `kill_agent`, `quit`) over a one-line-per-request protocol.

Unit tests cover the stat parser, adoption with a mismatched `start_time`, root selection (leader vs worker, orphaned
workers, `setsid` workers), exact marker matching and the shim status protocol.

## How to run

On a Linux machine, as a **non-root** user (this is the case that matters), from the repository root. Compile only this
crate, not the workspace:

```shell
cargo run -p poc-linux-crash-survival --example <example>
cargo test -p poc-linux-crash-survival
```

`crash_survival_cgroup` creates a cgroup under AC's own, which needs cgroup v2 and write access. As a regular user it
prints `SKIP: ...` and exits 0 unless the parent cgroup is delegated to you (e.g. `systemd-run --user --scope -p
Delegate=yes ...`). Run it as root to exercise it fully.

### From macOS (or anywhere without a Linux host)

The examples only run on Linux. With Docker, `--privileged` lets the cgroup example write to the cgroup filesystem and
`--init` runs an init as PID 1 that reaps the killed orphans. Without it they stay zombies; the PoC treats a zombie as gone,
but the cgroup cannot be removed until it is reaped.

```shell
docker run --rm --init --privileged -v "$PWD":/work -w /work -e CARGO_TARGET_DIR=/tmp/target rust:latest \
  cargo run -p poc-linux-crash-survival --example <example>
```

Inside the container you are root. To check the no-root claim run the same command as an unprivileged user (for example
`--user 1000:1000` with a writable `CARGO_HOME`), where the cgroup example skips.

## Limitations

- **Same user only.** The scan and the adoption consider processes of the same effective uid. Sub-agents running as another
  user would need extra privileges.
- **No exit status for orphans** in bookkeeping, env tag and cgroup: AC is not the parent, so `waitpid` is not possible. Only
  the shim keeps it.
- **stdio.** The supervisor spawns the sub-agent with no stdio attached. A real AC pipes stdout/stderr, which breaks when AC
  dies (`SIGPIPE` on the next write), so a real implementation must point them at something that outlives AC.
- **Simulated sub-agents.** `sleep`, a forking copy of the example and `env -i sh -c ...` stand in for real agents. Real
  agents may behave differently (re-exec themselves, rewrite `argv`, drop uid).
- **Env tag scan cost** grows with the number of processes on the host and was not measured here.
- The examples have only been compile-checked (`cargo check`/`clippy` for a Linux target) from macOS; see the PR/branch notes
  for run results.
