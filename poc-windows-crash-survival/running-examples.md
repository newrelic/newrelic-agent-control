# Running the PoC Examples — Quick Reference

All examples live in `poc-windows-crash-survival/examples/`. They must be compiled and run on
the Windows 11 VM; no cross-run from macOS.

**Authors**: examples marked *(prior spike)* were written by [@sigilioso](https://github.com/sigilioso)
before NR-645389; examples marked *(NR-645389)* were added in this spike.

---

## Setup (do once per session)

### Build all examples

```sh
prlctl exec "Windows 11" --current-user cmd /c "set CARGO_TARGET_DIR=C:\Users\davidsanchez\cargo-target&&cargo build -p poc-windows-crash-survival --examples --manifest-path E:\NR-repos\ac\poc-windows\Cargo.toml"
```

All binaries land in `C:\Users\davidsanchez\cargo-target\debug\examples\`.

### Run as SYSTEM vs non-elevated

| Mode | Command prefix |
| --- | --- |
| `NT AUTHORITY\SYSTEM` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\<name>.exe"` |
| Current interactive user (`davidsanchez`) | `prlctl exec "Windows 11" --current-user cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\<name>.exe"` |

> Note: `prlctl exec` without `--current-user` always runs as SYSTEM. The SMB drive letters
> (E:, Z:) are **not** available to SYSTEM — only `C:\` paths are reachable. The cargo target
> dir on `C:` is accessible from both modes.

---

## NR-645389 design questions → probes

The [NR-645389 design document](https://newrelic.atlassian.net/wiki/spaces/INST/pages/6418007106/)
answers eight questions. The table below maps each one to the example that provides the evidence,
with the exact `prlctl` command to reproduce it. Run [Setup](#setup-do-once-per-session) first.

| # | Question | Example | Command (run from macOS terminal) |
| --- | --- | --- | --- |
| Q1 | Job name composition; Session 0 = global namespace | `permanent_job_probe` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\permanent_job_probe.exe"` — look for `session: 0` and `\BaseNamedObjects\` (not `\Sessions\1\`) in the resolved name |
| Q2 | `SeCreatePermanentPrivilege` requirement | `permanent_job_probe` | same command; also run `permanent_job_probe.exe disable` to see FAIL with privilege held-but-disabled |
| Q3 | DACL at creation; squatting risk | `job_dacl_probe` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\job_dacl_probe.exe"` |
| Q4 | Verify owner and DACL on adoption | `crash_survival_permanent_job` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\crash_survival_permanent_job.exe"` |
| Q5 | Kill propagates to grandchildren; `TerminateProcess` vs `TerminateJobObject` | `sub_process_tree_probe` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\sub_process_tree_probe.exe"` |
| Q6 | Liveness check after adoption; recovery when sub-agent is dead | `leftover_and_restart_probe` | `prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\leftover_and_restart_probe.exe"` |
| Q7 | Job cleanup on sub-agent removal | `crash_survival_permanent_job` | same as Q4 — the cleanup step at the end of the example covers Q7 |
| Q8 | When is the leader PID still needed? | `sub_process_tree_probe` (Part B vs C) | same as Q5 — Part B shows `TerminateProcess` leaves children alive; Part C shows `terminate_all` clears the job |

---

## Quick reference table

| Example | Author | Privilege | What it tests |
| --- | --- | --- | --- |
| `crash_survival_bookkeeping` | prior spike | non-elevated | PID+start-time bookkeeping survives supervisor crash |
| `env_tag_probe` | prior spike | non-elevated | Env-var tag scan recovers a child without any stored handle |
| `job_name_probe` | prior spike | non-elevated | Temporary job name disappears when last handle closes |
| `permanent_job_probe` | prior spike | SYSTEM | `OBJ_PERMANENT` keeps name alive; `NtMakeTemporaryObject` removes it |
| `job_dacl_probe` | prior spike | SYSTEM | Default vs restrictive DACL; DACL survives handle closure |
| `list_jobs` | prior spike | SYSTEM | Directory scan finds permanent orphan with no open handles |
| `crash_survival_permanent_job` | prior spike | SYSTEM | End-to-end: crash → permanent job leftover → re-adopt from name alone |
| `leftover_and_restart_probe` | NR-645389 | SYSTEM | `OBJ_OPENIF` open-or-create; alive vs dead member on re-open |
| `name_squatting_probe` | NR-645389 | SYSTEM | Owner/DACL checks defend against temp and permanent squatters |
| `sub_process_tree_probe` | NR-645389 | SYSTEM | Auto-join, `TerminateProcess` orphans, `terminate_all`, breakaway |
| `adversarial_spawn_probe` | NR-645389 | SYSTEM | Spawn-race closure; `ActiveProcessLimit=0` is not a sentinel |

---

## Per-example instructions

### `crash_survival_bookkeeping` *(prior spike)*

**Run as non-elevated** (that is the whole point — no admin rights should be required):

```sh
prlctl exec "Windows 11" --current-user cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\crash_survival_bookkeeping.exe"
```

**Expected output:**

```console
elevated: false
children of dead supervisor <SUPERVISOR_PID>: [<CHILD_PID>] (child: <CHILD_PID>)
PASS
```

**What to look for:**

- `elevated: false` confirms this works without admin rights.
- The supervisor launches a `ping` child, writes `(pid, start_time)` to a temp file and
  calls `abort()` — simulating a crash. The restarted AC (the orchestrator) reads the file,
  re-opens the process, confirms the start time matches, and kills it.
- `children of dead supervisor …` shows the orphan still reports the crashed supervisor as
  its parent in the process snapshot — a snapshot scan can narrow recovery to children of the
  last-known AC pid without opening every process.

---

### `env_tag_probe` *(prior spike)*

**Run as non-elevated:**

```sh
prlctl exec "Windows 11" --current-user cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\env_tag_probe.exe"
```

**Expected output:**

```console
elevated: false
NR_AC_TAG env: Ok(Some("demo-tag"))
[full scan, no filter]
  inspected: <N>, skipped (unreadable): <M>, matches: [<PID>], took <DURATION>
[recovery by tag, image filter ["ping.exe"]]
  inspected: <SMALL>, skipped (unreadable): 0, filtered out unopened: <LARGE>, took <DURATION>
  matches: [<PID>] (original child: <PID>)
  pid <PID>: alive=Ok(true), start_time=Ok(<TIMESTAMP>)
  kill: Ok(())
  exited within 5s: true, alive=Ok(false)
```

**What to look for:**

- The full scan (`no filter`) opens every readable process to check `NR_AC_TAG` — expect
  hundreds of processes inspected and a slow duration.
- The filtered scan (`["ping.exe"]`) skips everything that isn't a ping — `filtered out
  unopened` will be large, `inspected` tiny, and the duration much shorter.
- `kill: Ok(())` confirms adoption and termination work without elevation.

---

### `job_name_probe` *(prior spike)*

**Run as non-elevated or SYSTEM** (behaviour differs only in which namespace `Local\` resolves to):

```sh
prlctl exec "Windows 11" --current-user cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\job_name_probe.exe"
```

**Expected output:**

```console
--- name: Local\nr-ac-probe-<PID>
assign: Ok(())
[handle held]
  via owned handle: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [...], name: Some("\\Sessions\\1\\BaseNamedObjects\\Local\\nr-ac-probe-<PID>") })
  via open by name: Ok(JobInfo { active_processes: 1, ... })
[last handle closed]
  child in some job: true (Ok(()))
  via open by name: opening named job Local\nr-ac-probe-<PID>: The system cannot find the file specified. (os error 2)
```

**What to look for:**

- While the handle is held the name is both queryable through the handle and openable by
  name. The `name: Some(...)` field shows the full NT path the kernel resolved it to.
- After the last handle closes the child is **still in the job** (`child in some job: true`)
  but the **name is gone** — `error 2` on open by name. This is why a temporary job cannot
  survive an AC crash: the name disappears with the handle.
- Run as SYSTEM (session 0) and the resolved name will be `\BaseNamedObjects\...` (no
  `Sessions\1\` prefix) because session 0 is the global namespace.

---

### `permanent_job_probe` *(prior spike)*

**Run as SYSTEM** (needs `SeCreatePermanentPrivilege`):

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\permanent_job_probe.exe"
```

Optional argument controls the privilege: `enable` (default), `disable`, `leave`.

**Expected output (SYSTEM, `enable` — default):**

```console
elevated: Ok(false), session: 0
SeCreatePermanentPrivilege held, after enable: Ok(true)
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job Global\nr-ac-perm-probe: The system cannot find the file specified. (os error 2)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, ... })
  permanent: made temporary, NTSTATUS 0x00000000
  permanent: after cleanup, open by name: opening named job Global\nr-ac-perm-probe: The system cannot find the file specified. (os error 2)
```

**Expected output (SYSTEM, `disable` — privilege held but disabled):**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\permanent_job_probe.exe disable"
```

```console
elevated: Ok(true), session: 0
SeCreatePermanentPrivilege held, after disable: Ok(true)
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
```

`Ok(true)` from `set_privilege(..., false)` confirms the privilege was present in the token;
`0xC0000061` (`STATUS_PRIVILEGE_NOT_HELD`) on the permanent create proves it must be
*enabled*, not merely present.

**Expected output (interactive user — Session 1, `Local\` namespace):**

```sh
prlctl exec "Windows 11" --current-user cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\permanent_job_probe.exe"
```

```console
elevated: Ok(false), session: 1
SeCreatePermanentPrivilege held, after enable: Ok(false)
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
--- session namespace: \Sessions\1\BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
```

`Ok(false)` from `set_privilege` confirms `SeCreatePermanentPrivilege` is not in the
interactive user's token (expected — no grant has been made). The session namespace block
appears only in Session 1 (`session != 0`); `Local\nr-ac-perm-probe` targets
`\Sessions\1\BaseNamedObjects\` — a distinct object namespace from `\BaseNamedObjects\`
which AC uses from Session 0.

**What to look for:**

- `session: 0` — SYSTEM always runs in session 0; the session namespace block does not appear.
- `enable` run: temporary job name vanishes after `CloseHandle`; permanent job survives and is queryable; `NtMakeTemporaryObject` removes it.
- `disable` run: `STATUS_PRIVILEGE_NOT_HELD` (`0xC0000061`) even though the privilege is present in the token — it must be *enabled* at call time.
- `--current-user` run (`session: 1`): session namespace block appears; `Local\` resolves to `\Sessions\1\BaseNamedObjects\`, a separate namespace from AC's `\BaseNamedObjects\`.

---

### `job_dacl_probe` *(prior spike)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\job_dacl_probe.exe"
```

**Expected output (SYSTEM, global namespace only):**

```console
elevated: Ok(false), session: 0
current user SID: S-1-5-18
SeCreatePermanentPrivilege held, after enable: Ok(true)
--- global namespace: \BaseNamedObjects\nr-ac-dacl-probe
  [default DACL]
    after creation (permanent: true), read from the handle:
      owner: S-1-5-32-544 (BUILTIN\Administrators)
      SDDL:  O:BAD:(A;;0x1f003f;;;SY)(A;;0x1f003f;;;BA)
      ACE:   Allow 0x001F003F S-1-5-18 (NT AUTHORITY\SYSTEM)
      ACE:   Allow 0x001F003F S-1-5-32-544 (BUILTIN\Administrators)
      audit: owner is the current user or Administrators
      audit: also granted to: ["BUILTIN\\Administrators mask 0x001f003f"]
    last handle closed, reopened by name with READ_CONTROL:
      [same output — DACL survived handle closure]
    cleanup: Ok(())
  [restrictive DACL]
    requested SDDL: D:P(A;;GA;;;SY)
    after creation (permanent: true), read from the handle:
      owner: S-1-5-32-544 (BUILTIN\Administrators)
      SDDL:  O:BAD:P(A;;0x1f003f;;;SY)
      ACE:   Allow 0x001F003F S-1-5-18 (NT AUTHORITY\SYSTEM)
      audit: owner is the current user or Administrators
      audit: only the current user and SYSTEM are granted access
    last handle closed, reopened by name with READ_CONTROL:
      [same output]
    cleanup: Ok(())
```

**What to look for:**

- **Default DACL**: the `audit: also granted to: ["BUILTIN\\Administrators …"]` line — the
  default DACL gives Administrators full access, which is too permissive.
- **Restrictive DACL** (`D:P(A;;GA;;;SY)`): only SYSTEM has an ACE. The `P` flag blocks
  inherited ACEs. The DACL is **stored on the object**, not the handle — reopening after
  `CloseHandle` returns the same SDDL, confirming it persists across crashes.
- The `SDDL` printed after reopening must match what was set at creation time.

---

### `list_jobs` *(prior spike)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\list_jobs.exe"
```

**Expected output:**

```console
elevated: Ok(false), session: 0
SeCreatePermanentPrivilege held: Ok(true)
--- \BaseNamedObjects
[all] jobs: <N>, unreadable jobs: 0, entries of other types: <M>
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[<PID>]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  ... (other system jobs)
[filter: name starts with "nr-ac-list-demo-", permanent only: false]
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[<PID>]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  matches: 2
  permanent jobs nobody holds a handle to: 1
cleanup: make_temporary(\BaseNamedObjects\nr-ac-list-demo-permanent-orphan): Ok(())
```

**What to look for:**

- `permanent=true other_handles=0 active_processes=1` — this is exactly what a crash
  leftover looks like from a restarted AC's point of view: the permanent job is still there,
  nobody holds a handle, but a sub-agent is still running inside it.
- `other-list-demo-decoy` does not appear in the filtered output — prefix filtering works.
- `permanent jobs nobody holds a handle to: 1` — a restarted AC can scan for exactly this
  condition to find its sub-agents' jobs.

---

### `crash_survival_permanent_job` *(prior spike)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\crash_survival_permanent_job.exe"
```

**Expected output:**

```console
elevated: false
SeCreatePermanentPrivilege held: true
supervisor <PID>: child <CHILD_PID> in \BaseNamedObjects\nr-ac-crash-survival-<ORCHESTRATOR_PID>
NamedJob { path: "\\BaseNamedObjects\\nr-ac-crash-survival-<PID>", name: "nr-ac-crash-survival-<PID>", permanent: true, other_handles: 0, active_processes: 1, pids: [<CHILD_PID>] }
owner: "S-1-5-32-544", sddl: "O:BAD:P(A;;0x1f003f;;;SY)"
members: [<CHILD_PID>]
cleanup: Ok(())
PASS
```

**What to look for:**

- The supervisor prints its pid and the child pid then crashes (`abort()`); all its handles close.
- The orchestrator finds the job with `other_handles=0` and `active_processes=1` — identical
  to what `list_jobs` called a "permanent orphan".
- Owner check (`S-1-5-32-544` = BUILTIN\Administrators) passes; DACL check finds no unexpected grants.
- Adoption, kill, and cleanup all succeed — this is the full end-to-end path.

---

### `leftover_and_restart_probe` *(NR-645389)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\leftover_and_restart_probe.exe"
```

**Expected output:**

```console
── Scenario A: member alive after crash ──
create_or_open: was_existing=true (STATUS_OBJECT_NAME_EXISTS=0x40000000)
owner: S-1-5-32-544, sddl: O:BAD:P(A;;0x1f003f;;;SY)
active_processes=1, pids=[<PID>]
PASS A

── Scenario B: member died while AC was down ──
create_or_open: was_existing=true
active_processes=0, pids=[]
empty job detected: sub-agent is not running, would re-launch
PASS B

PASS
```

**What to look for:**

- `was_existing=true` in both scenarios: `NtCreateJobObject` with `OBJ_OPENIF|OBJ_PERMANENT`
  returns `STATUS_OBJECT_NAME_EXISTS` (NTSTATUS `0x40000000`, not an error — bit 31 clear)
  instead of creating a fresh object. That is the signal that triggers adoption checks.
- Scenario A (`active_processes=1`): member is alive — adopt it, verify with `process.wait(200ms)`.
- Scenario B (`active_processes=0`): member exited while AC was down — re-launch into the
  existing job without cleanup.

---

### `name_squatting_probe` *(NR-645389)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\name_squatting_probe.exe"
```

**Expected output:**

```console
elevated: false
── Part 1: temporary squatter detected (OBJ_OPENIF opens existing object) ──
squatter created temporary job: \BaseNamedObjects\nr-ac-squatting-probe-1-<PID>
create_or_open: was_existing=true (NTSTATUS 0x40000000)
AC sees was_existing=true → verify owner before trusting (same path as Part 2)
PASS 1

── Part 2: permanent squatter detected (STATUS_OBJECT_NAME_EXISTS) ──
squatter created permanent job: \BaseNamedObjects\nr-ac-squatting-probe-2-<PID>
create_or_open: was_existing=true (NTSTATUS 0x40000000)
existing job: active_processes=0
AC sees was_existing=true → must verify owner before trusting
PASS 2

── Part 3: owner check (same user, permanent squatter) ──
owner: S-1-5-18 (NT AUTHORITY\SYSTEM)
unexpected_owner (allowing our SID): None
was_existing=true: job predates this startup, even though owner matches
PASS 3

── Part 4: owner check rejects unexpected owner (cross-user simulation) ──
actual owner: S-1-5-32-544
unexpected_owner (allowing only S-1-1-0=Everyone): Some("BUILTIN\\Administrators")
cross-user squatter would be caught: owner mismatch → AC refuses adoption
PASS 4

── Part 5: unguessable name ──
deterministic + unguessable name: \BaseNamedObjects\nr-ac-infrastructure-<128-bit-hex>
was_existing=false: name was free, no squatting possible
PASS 5

PASS
```

**What to look for:**

- Parts 1 and 2: both a *temporary* squatter (no privilege needed) and a *permanent* squatter
  produce the same `was_existing=true` signal. The attack surface is unified.
- Part 3: same-user squatter — owner check passes, but `was_existing=true` lets AC refuse
  to adopt a job it did not create in this run.
- Part 4: cross-user squatter — `unexpected_owner` returns the mismatch, AC refuses adoption.
- Part 5: a name containing a 128-bit timestamp makes squatting impractical without advance
  knowledge of the name.

---

### `sub_process_tree_probe` *(NR-645389)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\sub_process_tree_probe.exe"
```

**Expected output:**

```console
── Part A: children of a job member auto-join the job ──
job has 3 active processes: [<PID>, <PID>, <PID>]
active_processes=3 (sub-agent + 2 children), all in job
PASS A

── Part B: TerminateProcess on top-level leaves children alive ──
before=3 active, after TerminateProcess on top-level: 2 (children alive)
PASS B

── Part C: terminate_all kills every member including children ──
before=3, after terminate_all: 0
PASS C

── Part D: CREATE_BREAKAWAY_FROM_JOB requires JOB_OBJECT_LIMIT_BREAKAWAY_OK ──
  without BREAKAWAY_OK:
    sub-agent-breakaway exit code: Some(5)
  with BREAKAWAY_OK:
    sub-agent-breakaway exit code: Some(0)
PASS D

PASS
```

**What to look for:**

- Part A: `active_processes=3` even though only the sub-agent was explicitly assigned — the
  two `ping` children joined automatically (Windows 8+ nested job support, no action needed).
- Part B: killing only the sub-agent drops count by 1 — the two ping children become orphans
  still tracked by the job. `TerminateProcess` on the root is **not** enough to clean up.
- Part C: `terminate_all` (= `TerminateJobObject`) kills everything; count goes to 0.
- Part D: `exit code: Some(5)` = `ERROR_ACCESS_DENIED` when `BREAKAWAY_OK` is absent.
  After `allow_breakaway(true)`, exit code 0. AC should never set `BREAKAWAY_OK` unless it
  deliberately allows the sub-agent to escape the job.

---

### `adversarial_spawn_probe` *(NR-645389)*

**Run as SYSTEM:**

```sh
prlctl exec "Windows 11" cmd /c "C:\Users\davidsanchez\cargo-target\debug\examples\adversarial_spawn_probe.exe"
```

**Expected output:**

```console
── Part A: flat-spawner + limit=1 + terminate_all ──
job has 4 active processes
before limit: active=4
limit=1 set; spawner's next spawn attempts will get ERROR_NOT_ENOUGH_QUOTA
after 1 s with limit=1: active=3 (existing processes not killed)
active_processes=0
PASS A

── Part B: chain-spawner(2) tree + kill root + limit=0 + terminate_all ──
job has 6 active processes
before killing root: active=6
after killing root pid=<PID>: active=5 (subtree survives; orphaned spawners still active)
count grew 5 → 9 while root was dead — orphaned chain-spawners are still independently spawning
before terminate: active=9
limit=0 set
after 1 s: active=6 — limit=0 appears to gate new spawns ('no processes allowed')
active_processes=0 — converged
verdict: limit=0 gated new spawns; same guarantees as limit=1
PASS B

PASS
```

**What to look for:**

- Part A: `active=3` after 1 s (not 4) — existing ping children expired naturally while the
  spawner's new spawns were silently failing with `ERROR_NOT_ENOUGH_QUOTA`. The limit does not
  kill existing processes. The key assertion is that `terminate_all` drains to 0 cleanly.
- Part B, step 1 (kill root only): count dropped by 1 (just the root) but then **grew** while
  the root was dead — orphaned subtree nodes continued spawning independently. The `count grew
  5 → 9` line is the concrete evidence.
- Part B, step 2 (limit=0): count drops after the gate is set (no new pings can join) and
  reaches 0 after `terminate_all`. The verdict line tells you whether limit=0 is a hard gate
  (as seen on Windows 11) or a sentinel meaning unlimited. On Windows 11: **not a sentinel**.
- If the `count grew` line does not appear (Part B), some pings may have expired before the
  1 s observation window — the test is still valid as long as both parts pass.

---

## Troubleshooting

**`error 3` / `The system cannot find the path specified`** when building:
The trailing-space issue — never put a space before `&&` in `set VAR=value && command`.
Use `set VAR=value&&command` (no spaces around `&&`).

**`SeCreatePermanentPrivilege` not held** on an example that needs it:
The example is not running as SYSTEM. Drop `--current-user` from the `prlctl exec` call.

**`NTSTATUS 0xC0000061`** on a permanent-job create:
`STATUS_PRIVILEGE_NOT_HELD`. Same cause as above, or the privilege was not enabled with
`AdjustTokenPrivileges`.

**`NTSTATUS 0xC0000035`** (`STATUS_OBJECT_NAME_COLLISION`):
A leftover permanent job from a crashed run is blocking the name, **and** the code is
missing `OBJ_OPENIF`. In the current codebase this should not happen — `create_or_open_native`
always uses `OBJ_PERMANENT | OBJ_OPENIF`. If you see it in custom code, add `OBJ_OPENIF`.
Clean up a leftover manually with `list_jobs.exe --no-demo` to find it, then re-run the
example (it calls `make_temporary` at the end).
