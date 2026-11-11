# NR-645389 Spike Notes — Permanent Named Jobs on Windows

Working notes for the spike. Each question from the ticket is answered in turn; findings are
backed by running the examples against a Parallels Windows 11 VM as `NT AUTHORITY\SYSTEM`
(via `prlctl exec "Windows 11" cmd /c ...`) unless noted otherwise.

Status legend: ✅ answered | 🔄 in progress | ⬜ pending

---

## Q1 — How can the job name be composed?

**Status: ✅**

The name is an NT object manager path. The right form is:

```
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

**Status: ✅**

Verified by `job_name_probe` (both as non-elevated user and as SYSTEM):

| Namespace prefix | Resolves to | Visible from session 0 (AC service)? |
|---|---|---|
| `Local\` | `\Sessions\<n>\BaseNamedObjects` | No — per-session |
| `Global\` | `\BaseNamedObjects` | Yes |
| No prefix (bare `\BaseNamedObjects\...`) | `\BaseNamedObjects` | Yes — same as `Global\` |

AC should use bare `\BaseNamedObjects\...` paths (or `Global\`) so the job is reachable from
the service session (session 0) regardless of which session created it. The `job_name_probe`
output confirmed that a job created by SYSTEM resolves in `\BaseNamedObjects` directly, while
the same name created by an interactive user resolves under `\Sessions\1\BaseNamedObjects`.

---

## Q3 — Are the required privilege grants acceptable for the non-elevated user case?

**Status: ✅**

The privilege is `SeCreatePermanentPrivilege` ("Create permanent shared objects"). The ticket
mentions `SeChangeNotifyPrivilege` but that is a copy-paste error — `SeChangeNotify` is granted
to all users by default and does not gating anything here.

Verified by `permanent_job_probe` and `crash_survival_permanent_job`:

| Identity | Privilege in token | Can create permanent job? |
|---|---|---|
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

**Status: ✅**

Verified by `job_dacl_probe`. The job must be created with an explicit SDDL passed in
`OBJECT_ATTRIBUTES.SecurityDescriptor` to `NtCreateJobObject`; a `SetSecurityInfo` call
afterwards leaves a window where the default DACL is in effect.

Recommended SDDL:

```
D:P(A;;GA;;;SY)(A;;GA;;;<AC-user-SID>)
```

| Part | Meaning |
|---|---|
| `D:P` | DACL, protected (no inherited ACEs) |
| `(A;;GA;;;SY)` | `GENERIC_ALL` for `SYSTEM` |
| `(A;;GA;;;<user-SID>)` | `GENERIC_ALL` for the AC service account (omit when AC is SYSTEM, it would duplicate the above) |

The output after creation and after reopening by name matched — the DACL is stored on the object
and survives handle closure:

```
SDDL:  O:BAD:P(A;;0x1f003f;;;SY)
ACE:   Allow 0x001F003F S-1-5-18 (NT AUTHORITY\SYSTEM)
audit: only the current user and SYSTEM are granted access
```

For supportability, `(A;;0x120004;;;BA)` (query + read DACL for `BUILTIN\Administrators`) can
be added when AC runs as SYSTEM, allowing admins to inspect the job without being able to
terminate it. This is a trade-off; see the README for the full analysis.

---

## Q5 — Can we verify the owner and/or DACL when "adopting" an existent Permanent Job?

**Status: ✅**

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

```
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
|---|---|---|---|
| Temporary job (no privilege needed) | `STATUS_OBJECT_NAME_EXISTS` + `was_existing=true` | Yes — owner check runs | Owner check; unguessable name as secondary mitigation |
| Permanent job (`SeCreatePermanentPrivilege` required) | Same | Yes | Same |
| Same-user squatter | Owner check passes (same SID) | `was_existing=true` signals job predates this startup | Refuse adoption of any `was_existing=true` job AC did not create in this run |
| Cross-user squatter | Owner check catches mismatch | `unexpected_owner` returns the actual SID | Refuse adoption |

From `name_squatting_probe` output (SYSTEM run):

```
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

**Status: ⬜ pending validation**

The existing probes use `ping.exe` which does not spawn children. The question is whether
terminating the job (or just the top-level process) also cleans up any grandchildren.

Things to check:
- Does `TerminateProcess` on the adopted top-level handle also kill its descendants?
- Alternatively, does assigning a process to a job that has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
  clear cause descendants to also join the job (if `CREATE_BREAKAWAY_FROM_JOB` is not set)?
- What happens if a grandchild sets `CREATE_BREAKAWAY_FROM_JOB` at spawn?

---

## Q7 — Can we check if the sub-agent is running on adoption? What if it is not?

**Status: ✅**

Verified by `leftover_and_restart_probe` Scenario B (member died while AC was down):

```
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

```
── Scenario A: member alive after crash ──
create_or_open: was_existing=true (STATUS_OBJECT_NAME_EXISTS=0x40000000)
owner: S-1-5-32-544, sddl: O:BAD:P(A;;0x1f003f;;;SY)
active_processes=1, pids=[10404]
PASS A
```

---

## Q8 — How is "Job cleanup" implemented when an agent is removed?

**Status: ✅**

`make_temporary(nt_path)` is the cleanup call. It opens the job with `DELETE` access and calls
`NtMakeTemporaryObject`, which clears the permanent attribute. The name disappears from the
object directory as soon as the last handle to the job closes (either immediately, if no handles
are currently open, or when AC drops its own handle).

Verified in every example that creates a permanent job — `crash_survival_permanent_job`,
`leftover_and_restart_probe`, `name_squatting_probe`:

```
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
