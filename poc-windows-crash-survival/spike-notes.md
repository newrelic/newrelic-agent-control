# NR-645389 Spike Notes — Permanent Named Jobs on Windows

Working notes for the spike. Each question from the ticket is answered in turn; findings are
backed by running the examples against a Parallels Windows 11 VM as `NT AUTHORITY\SYSTEM`
(via `prlctl exec "Windows 11" cmd /c ...`) unless noted otherwise.

Status legend: ✅ answered | 🔄 in progress | ⬜ pending

> **How to run the examples**: see [`running-examples.md`](running-examples.md) for build
> commands, per-example run instructions, expected output and troubleshooting tips.

---

## Q1 — How can the job name be composed?

- **Status: ✅**

The name is an NT object manager path. The right form is:

```txt
\BaseNamedObjects\nr-ac-<sub-agent-id>
```

- `\BaseNamedObjects` is the global namespace, accessible from session 0 (where AC runs as a
  Windows service). Names there are visible across all sessions. Using `Local\` would resolve
  to `\Sessions\<n>\BaseNamedObjects`, which is per-session and cannot be opened from another
  session (see Q2).
- The sub-agent ID (from AC config) makes the name deterministic across restarts: the restarted
  AC derives the same name without any stored state.
- A per-instance suffix (e.g. first-spawn timestamp as hex) can be appended for same-user
  squatting mitigation (see Q5), at the cost of losing determinism across re-installs.

---

## Q2 — Are Windows namespaces meaningful (`Local\`, `Global\`, …)?

- **Status: ✅**

Verified by `job_name_probe` (both as non-elevated user and as SYSTEM):

| Namespace prefix | Resolves to | Visible from session 0 (AC service)? |
| --- | --- | --- |
| `Local\` | `\Sessions\<n>\BaseNamedObjects` | No — per-session |
| `Global\` | `\BaseNamedObjects` | Yes |
| No prefix (bare `\BaseNamedObjects\...`) | `\BaseNamedObjects` | Yes — same as `Global\` |

AC should use bare `\BaseNamedObjects\...` paths (or `Global\`) so the job is reachable from
the service session (session 0) regardless of which session created it. The `job_name_probe`
output confirmed that a job created by SYSTEM resolves in `\BaseNamedObjects` directly, while
the same name created by an interactive user resolves under `\Sessions\1\BaseNamedObjects`.

---

## Q3 — Are the required privilege grants acceptable for the non-elevated user case?

- **Status: ✅**

The privilege is `SeCreatePermanentPrivilege` ("Create permanent shared objects"). The ticket
mentions `SeChangeNotifyPrivilege` but that is a copy-paste error — `SeChangeNotify` is granted
to all users by default and does not gating anything here.

Verified by `permanent_job_probe` and `crash_survival_permanent_job`:

| Identity | Privilege in token | Can create permanent job? |
| --- | --- | --- |
| `NT AUTHORITY\SYSTEM` (current `install.ps1`) | Present and enabled by default | Yes |
| Non-elevated administrator (`prlctl --current-user`) | Not present | No (`0xC0000061`) |
| Standard user `nrpoc` before grant | Not present | No (`0xC0000061`) |
| Standard user `nrpoc` after admin grants the right via `secpol.msc` / `secedit` | Present but **disabled** — AC must call `AdjustTokenPrivileges` at startup | Yes |

Key points:

- The privilege must be both **present and enabled** in the token. Granting it via policy adds
  it disabled; the process must enable it with `AdjustTokenPrivileges`.
- For AC running as `SYSTEM` (the current default) there is nothing to configure.
- For a non-admin AC service account, an administrator must grant the right once at install
  time. This is a security-sensitive policy change (it allows creating permanent kernel objects),
  so it should go to a dedicated service account, not to all users.
- Acceptability judgement: reasonable for a service account; AC's installer already runs as
  admin and can set this via `secedit` or `LsaAddAccountRights`.

---

## Q4 — What security descriptor should we use to create the Permanent Job?

- **Status: ✅**

Verified by `job_dacl_probe`. The job must be created with an explicit SDDL passed in
`OBJECT_ATTRIBUTES.SecurityDescriptor` to `NtCreateJobObject`; a `SetSecurityInfo` call
afterwards leaves a window where the default DACL is in effect.

Recommended SDDL:

```txt
D:P(A;;GA;;;SY)(A;;GA;;;<AC-user-SID>)
```

| Part | Meaning |
| --- | --- |
| `D:P` | DACL, protected (no inherited ACEs) |
| `(A;;GA;;;SY)` | `GENERIC_ALL` for `SYSTEM` |
| `(A;;GA;;;<user-SID>)` | `GENERIC_ALL` for the AC service account (omit when AC is SYSTEM, it would duplicate the above) |

The output after creation and after reopening by name matched — the DACL is stored on the object
and survives handle closure:

```txt
SDDL:  O:BAD:P(A;;0x1f003f;;;SY)
ACE:   Allow 0x001F003F S-1-5-18 (NT AUTHORITY\SYSTEM)
audit: only the current user and SYSTEM are granted access
```

For supportability, `(A;;0x120004;;;BA)` (query + read DACL for `BUILTIN\Administrators`) can
be added when AC runs as SYSTEM, allowing admins to inspect the job without being able to
terminate it. This is a trade-off; see the README for the full analysis.

---

## Q5 — Can we verify the owner and/or DACL when "adopting" an existent Permanent Job?

- **Status: ✅**

Verified by `name_squatting_probe` (all 5 parts pass as SYSTEM) and `leftover_and_restart_probe`
(both scenarios pass).

### The mechanism

`NtCreateJobObject` with `OBJ_OPENIF | OBJ_PERMANENT` on an already-existing name returns
`STATUS_OBJECT_NAME_EXISTS` (NTSTATUS `0x40000000`, an info/warning — bit 31 clear) instead of
creating a new object. The `create_or_open_native` function in `lib.rs` maps this to
`was_existing: true` in the returned `JobCreation`.

**`OBJ_OPENIF` is required.** Without it `NtCreateJobObject` on any existing name — whether the
object is temporary or permanent — returns `STATUS_OBJECT_NAME_COLLISION` (error), not
`STATUS_OBJECT_NAME_EXISTS`. This was the initial bug in the implementation; the fix is
`OBJ_PERMANENT | OBJ_OPENIF` in the attributes.

### The adoption flow

```txt
create_or_open_native(name, sddl)
  ├─ was_existing = false  →  AC created this job now; no verification needed
  └─ was_existing = true   →  pre-existing; run verification before trusting
       ├─ job.security()   →  owner SID + full SDDL
       ├─ unexpected_owner(&[ac_user_sid, "S-1-5-32-544"])
       │    Some(_)  →  owner mismatch → refuse adoption
       │    None     →  owner is trusted
       └─ unexpected_grants(&[ac_user_sid, "S-1-5-18"])
            non-empty  →  unexpected ACE → refuse adoption
            empty      →  DACL is clean; proceed
```

### Squatting scenarios

| Squatter type | `NtCreateJobObject` result | Detection | Defence |
| --- | --- | --- | --- |
| Temporary job (no privilege needed) | `STATUS_OBJECT_NAME_EXISTS` + `was_existing=true` | Yes — owner check runs | Owner check; unguessable name as secondary mitigation |
| Permanent job (`SeCreatePermanentPrivilege` required) | Same | Yes | Same |
| Same-user squatter with wrong/default DACL | Owner check passes (same SID); DACL check fails | `unexpected_grants` returns unexpected ACEs | Refuse adoption |
| Same-user squatter with correct DACL | Both checks pass | Indistinguishable from legitimate adoption at the kernel-object level | Unguessable name (UUID suffix) closes the pre-emption window; attacker running as the service account has broader access anyway |
| Cross-user squatter | Owner check catches mismatch | `unexpected_owner` returns the actual SID | Refuse adoption |

Note: `was_existing=true` alone is **not** a squatting signal — it is the normal adoption case (AC restarted, sub-agent still running, job orphaned). The owner+DACL checks are what distinguish a legitimate orphaned job from a hostile one.

From `name_squatting_probe` output (SYSTEM run):

```txt
── Part 1: temporary squatter detected (OBJ_OPENIF opens existing object) ──
squatter created temporary job: \BaseNamedObjects\nr-ac-squatting-probe-1-7340
create_or_open: was_existing=true (NTSTATUS 0x40000000)
PASS 1

── Part 4: owner check rejects unexpected owner (cross-user simulation) ──
actual owner: S-1-5-32-544
unexpected_owner (allowing only S-1-1-0=Everyone): Some("BUILTIN\\Administrators")
cross-user squatter would be caught: owner mismatch → AC refuses adoption
PASS 4
```

---

## Q6 — Does killing the Job work when the sub-agent spawns its own sub-processes?

- **Status: ✅**

Verified by `sub_process_tree_probe` (all four parts pass as SYSTEM). The sub-agent mode of the
example spawns two `ping -n 300` children; the orchestrator verifies membership and tests kill
strategies.

### Part A — Children auto-join the job

A process's children inherit job membership by default (Windows 8+, with nested job support).
No explicit assignment is needed; the kernel adds them automatically.

```txt
job has 3 active processes: [6664, 8932, 9936]
active_processes=3 (sub-agent + 2 children), all in job
PASS A
```

### Part B — `TerminateProcess` on the top-level leaves children alive

Killing only the sub-agent drops `active_processes` from 3 to 2. The two grandchildren stay
in the job as orphans. **AC must not use `TerminateProcess` alone to clean up a sub-agent whose
children matter.**

```txt
before=3 active, after TerminateProcess on top-level: 2 (children alive)
PASS B
```

### Part C — `job.terminate_all()` (`TerminateJobObject`) kills the entire tree

All members are terminated atomically; `active_processes` goes to 0.

```txt
before=3, after terminate_all: 0
PASS C
```

`TerminateJobObject` is the correct primitive for AC to use when decommissioning a sub-agent
or force-killing after a graceful-shutdown timeout.

### Addendum — safe shutdown sequence when a sub-agent spawns its own children

Verified by `adversarial_spawn_probe` (both parts pass as SYSTEM).

`TerminateJobObject` returns before all processes are confirmed dead. In the window between
`TerminateProcess` delivery and actual thread termination, a dying process can still call
`CreateProcess`. If no limit is set, that child joins the job and was never sent a kill — a
race that can leave `active_processes > 0` even after `terminate_all` returns.

The safe sequence is:

```txt
1. job.limit_active_processes(N)   // N ≤ current count — gate: any new spawn fails immediately
2. job.terminate_all(exit_code)    // kills all current members
3. wait active_processes == 0      // guaranteed: gate closed, TerminateProcess is unconditional
```

`adversarial_spawn_probe` Part A verified this with a flat-loop spawner and `N=1`:

```txt
job has 4 active processes
before limit: active=4
limit=1 set; spawner's next spawn attempts will get ERROR_NOT_ENOUGH_QUOTA
after 1 s with limit=1: active=3 (existing processes not killed)
active_processes=0
PASS A
```

`active=3` after 1 s (not 4): short-lived ping children expired naturally while new spawn
attempts were silently failing — existing processes are unaffected by the limit.

### Addendum — orphaned subtree keeps spawning; `ActiveProcessLimit=0` is not a sentinel

Verified by `adversarial_spawn_probe` Part B (chain-spawner tree, as SYSTEM).

A `chain-spawner(depth)` immediately spawns one `chain-spawner(depth-1)` child on startup,
then loops spawning pings. This models a sub-agent with long-lived children that are
themselves independently active.

**Killing only the root does not clean up the subtree — and the subtree keeps growing:**

```txt
job has 6 active processes
before killing root: active=6
after killing root pid=2816: active=5 (subtree survives; orphaned spawners still active)
count grew 5 → 9 while root was dead — orphaned chain-spawners are still independently spawning
```

After the root was gone, the depth-1 and depth-0 chain-spawners accumulated more ping
children — count went from 5 to 9. **AC cannot assume that killing the sub-agent's main
process cleans up its process tree.**

**`ActiveProcessLimit=0` is not a sentinel — it gates new spawns completely:**

```txt
before terminate: active=9
limit=0 set
after 1 s: active=6 — limit=0 appears to gate new spawns ('no processes allowed')
active_processes=0 — converged
verdict: limit=0 gated new spawns; same guarantees as limit=1
```

After setting limit=0, the count dropped from 9 to 6 in 1 s because existing pings expired
and no new ones could join. `terminate_all` then drained to 0.

`limit=0` is therefore safe to use as the shutdown gate: no new process can join at all,
which eliminates the spawn race entirely. AC can use `limit_active_processes(0)` as the
strictest possible pre-termination gate.

### Part D — `CREATE_BREAKAWAY_FROM_JOB` requires `JOB_OBJECT_LIMIT_BREAKAWAY_OK`

The test is proxied through a `sub-agent-breakaway` sub-command (the orchestrator is not in
the job, so only a job member can test the flag):

- **Without `JOB_OBJECT_LIMIT_BREAKAWAY_OK`** (the default for a job with no limits):
  `CreateProcess` with `CREATE_BREAKAWAY_FROM_JOB` from inside the job returns
  `ERROR_ACCESS_DENIED`. Exit code 5.
- **After `job.allow_breakaway(true)`** (sets `JOB_OBJECT_LIMIT_BREAKAWAY_OK` via
  `SetInformationJobObject` / `JobObjectExtendedLimitInformation`):
  the spawn succeeds. Exit code 0.

```txt
without BREAKAWAY_OK: sub-agent-breakaway exit code: Some(5)
with BREAKAWAY_OK:    sub-agent-breakaway exit code: Some(0)
PASS D
```

**Implication for AC**: as long as AC does not set `JOB_OBJECT_LIMIT_BREAKAWAY_OK`, the
sub-agent cannot escape the job with `CREATE_BREAKAWAY_FROM_JOB`. A well-behaved sub-agent
that needs to spawn outside the job would require AC to explicitly opt in. A misbehaving
sub-agent that calls `CreateProcess` with `CREATE_BREAKAWAY_FROM_JOB` will get
`ERROR_ACCESS_DENIED` and fail to do so.

Note: `SetInformationJobObject` with `JobObjectBasicLimitInformation` rejects the
`BREAKAWAY_OK` flag on Windows 11 with `ERROR_INVALID_PARAMETER`; the extended info class
(`JobObjectExtendedLimitInformation` / `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`) must be used.

---

## Q7 — Can we check if the sub-agent is running on adoption? What if it is not?

- **Status: ✅**

Verified by `leftover_and_restart_probe` Scenario B (member died while AC was down):

```txt
── Scenario B: member died while AC was down ──
create_or_open: was_existing=true
active_processes=0, pids=[]
empty job detected: sub-agent is not running, would re-launch
PASS B
```

The check is `job.info().active_processes`:

- `> 0` with PIDs → member is alive; adopt via `AdoptedProcess::open(pid)`
- `== 0` → member exited while AC was down; re-launch the sub-agent and assign it to the
  same job (the permanent name is already there, no cleanup needed before re-launch)

For the alive case (Scenario A), `AdoptedProcess::open(pid)` + `process.wait(200ms)` confirms
the process is still running before AC calls into it:

```txt
── Scenario A: member alive after crash ──
create_or_open: was_existing=true (STATUS_OBJECT_NAME_EXISTS=0x40000000)
owner: S-1-5-32-544, sddl: O:BAD:P(A;;0x1f003f;;;SY)
active_processes=1, pids=[10404]
PASS A
```

---

## Q8 — How is "Job cleanup" implemented when an agent is removed?

- **Status: ✅**

`make_temporary(nt_path)` is the cleanup call. It opens the job with `DELETE` access and calls
`NtMakeTemporaryObject`, which clears the permanent attribute. The name disappears from the
object directory as soon as the last handle to the job closes (either immediately, if no handles
are currently open, or when AC drops its own handle).

Verified in every example that creates a permanent job — `crash_survival_permanent_job`,
`leftover_and_restart_probe`, `name_squatting_probe`:

```txt
cleanup: make_temporary(\BaseNamedObjects\nr-ac-crash-survival-4104): Ok(())
```

Requirements for cleanup:

- The handle used for `make_temporary` must have `DELETE` access. `JOB_OBJECT_ALL_ACCESS`
  covers it; a read-only handle (e.g. `JOB_OBJECT_QUERY` only) would fail.
- AC must kill or wait for the sub-agent's process before (or after) calling `make_temporary`.
  The call does not terminate the members; it just removes the name.
- After `make_temporary` the job object itself lives until all handles close. AC's handle
  is the last one if the sub-agent has already exited.
- A permanent job with no open handles (leftover from a crash) can be cleaned up by opening
  it with `NtOpenJobObject(DELETE | JOB_OBJECT_QUERY)` and calling `make_temporary`. This is
  what `make_temporary` in `lib.rs` does.
