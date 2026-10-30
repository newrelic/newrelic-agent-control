# poc-windows-crash-survival

Standalone PoC to check whether, on Windows, a sub-agent can **survive an Agent Control (AC) crash** and be
**re-adopted by the restarted AC**, without elevation.

It is not wired into the AC binaries. It only lives in the workspace to reuse the pinned `windows` crate.

## The idea

AC would put every sub-agent in a Job Object. By default the kernel kills all the members of a job when the last
handle to it closes (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). This PoC does the opposite:

1. The job has **no limits**, in particular no kill-on-close. If AC crashes, the OS closes the job handle and the
   sub-agent keeps running. The default path uses an anonymous job, but a name is an option, see
   [Named Job Objects](#named-job-objects-why-a-plain-name-is-not-enough).
2. At spawn time AC **bookkeeps** `(pid, start_time)` of the sub-agent (or tags it, see
   [Discovery strategies](#discovery-strategies)).
3. The restarted AC finds the process again, checks it is the same one (`start_time` guards against PID reuse) and
   **adopts** it: it opens a handle with `SYNCHRONIZE | QUERY_LIMITED_INFORMATION | TERMINATE`, which is enough to
   wait on it, check it is alive and kill it.

With the default (bookkeeping) strategy the job only groups the sub-agent with its descendants while AC is alive and
is not used to find the process again. A permanent named job is the alternative where the job itself is the
discovery mechanism.

## What each example probes

| Example | Question it answers |
|---|---|
| `crash_survival_bookkeeping` | Does the child survive an AC crash, can it be adopted from `(pid, start_time)` and killed, all without elevation? Does a wrong `start_time` get rejected? Does the orphan keep the dead supervisor as parent pid? |
| `crash_survival_permanent_job` | End to end with a **permanent named job** as the only record: after the crash, can the restarted AC find the orphaned job by name, verify owner and DACL, adopt the members from the job (no `start_time`), kill them and clean the name up? |
| `job_name_probe` | Can a **named** job be used to find the sub-agent after a crash? What is still observable once the last handle is closed? |
| `permanent_job_probe` | Can a named job be made to **survive the loss of its handles** with `OBJ_PERMANENT`, and who is allowed to do that (non-elevated user vs `SYSTEM`)? |
| `job_dacl_probe` | What DACL does a permanent job get by default, does it persist after the last handle closes, and how is a restrictive one created atomically with the job? |
| `list_jobs` | How can a restarted AC **list and filter** the named jobs, find permanent ones that nobody holds a handle to, and what can a non-admin see? |
| `env_tag_probe` | Can the sub-agent be found by an environment variable tag (`NR_AC_TAG`) read from the process environment block, with no bookkeeping? What does the scan cost and how many processes are unreadable? |

Library pieces (`src/lib.rs`): `Job` (create/assign/query), `AdoptedProcess` (open, adopt with start time check, wait,
kill), `process_env_var` / `find_processes_by_env_tag` (remote PEB read), `find_processes_by_parent` (snapshot only)
`is_elevated`, `set_privilege`, and the native job helpers `Job::create_native`, `list_named_jobs` and
`make_temporary`. For DACLs: `Job::create_native_with_sddl`, `Job::open_native`, `Job::security` (owner, SDDL and ACEs),
`ObjectSecurity::unexpected_grants` (audit), `restrictive_sddl` and `current_user_sid`. Four unit tests cover adoption
after the job handle is dropped, the start time mismatch, listing a named job and a restrictive DACL on a job.

## How to run

The commands below were run from macOS against a Parallels Windows 11 VM. **Compile only this crate, not the workspace**:

```shell
prlctl exec "Windows 11" --current-user cmd /c "set CARGO_TARGET_DIR=C:\Users\calvarez\poc-target&& cd /d Y:\github.com\newrelic\newrelic-agent-control && cargo run -p poc-windows-crash-survival --example <example>"
```
On a Windows machine directly: `cargo run -p poc-windows-crash-survival --example <example>`.

### Test environment

```
> whoami
christianfed889\calvarez
> whoami /groups   (relevant lines)
BUILTIN\Administrators   S-1-5-32-544   Group used for deny only
BUILTIN\Users            S-1-5-32-545   Mandatory group, Enabled by default, Enabled group
Mandatory Label\Medium Mandatory Level
```

`prlctl --current-user` gives a **filtered (non-elevated) token**: Administrators is *deny only* and the integrity
level is Medium. `is_elevated()` returns `false` in those runs. See [Limitations](#limitations) for what this does and
does not prove.

The `LocalSystem` runs omit `--current-user`, which makes Parallels Tools execute as SYSTEM. The `Y:` mapping is per
user, so the already built binaries are run directly (build them first with the command above):

```shell
prlctl exec "Windows 11" cmd /c "C:\Users\calvarez\poc-target\debug\examples\<example>.exe"
> whoami
nt authority\system
> whoami /groups   (relevant line)
Mandatory Label\System Mandatory Level   S-1-16-16384
```

#### Setting up the `nrpoc` user

The runs under the standard user `nrpoc` (`permanent_job_probe`, `list_jobs`, `crash_survival_permanent_job`) need a
local account that is **not** an administrator. Everything below is done manually from an elevated PowerShell or
`cmd`.

1. **Create the user** (standard, `Users` group only):

   ```powershell
   net user nrpoc <password> /add
   net localgroup Users          # nrpoc should be listed (net user adds it by default)
   net localgroup Administrators # nrpoc must NOT be listed
   ```

2. **Make the binaries reachable.** `nrpoc` cannot read `C:\Users\calvarez`, so copy the already built examples to a
   shared folder and grant it read and execute:

   ```powershell
   mkdir C:\poc
   copy C:\Users\calvarez\poc-target\debug\examples\*.exe C:\poc\
   icacls C:\poc /grant "nrpoc:(OI)(CI)RX"
   ```

   Only `crash_survival_permanent_job` needs the helper `ping.exe`, which is already on the `PATH` for every user.

3. **Check the baseline** (the right is not granted yet). Open a new logon as `nrpoc` and list its privileges:

   ```
   runas /user:nrpoc cmd
   > whoami /priv
   ```

   `SeCreatePermanentPrivilege` must not be in the list. Running `permanent_job_probe.exe` here fails with
   `0xC0000061`, as in the baseline results below.

4. **Grant "Create permanent shared objects"** (`SeCreatePermanentPrivilege`) to `nrpoc`. With the GUI:

   - Run `secpol.msc`.
   - Go to *Local Policies > User Rights Assignment > Create permanent shared objects*.
   - *Add User or Group...*, type `nrpoc`, *OK*, *Apply*.

   Or from the command line with `secedit`:

   ```powershell
   $sid = (New-Object System.Security.Principal.NTAccount("nrpoc")).Translate([System.Security.Principal.SecurityIdentifier]).Value
   secedit /export /cfg C:\poc\sec.inf /areas USER_RIGHTS
   ```

   Edit `C:\poc\sec.inf`: under `[Privilege Rights]`, append `*<sid>` to the `SeCreatePermanentPrivilege` line (add the
   line if it is missing, entries are comma separated). Keep the `[Unicode]`, `[Version]` sections as exported. Then
   apply it:

   ```powershell
   secedit /configure /db C:\poc\sec.sdb /cfg C:\poc\sec.inf /areas USER_RIGHTS
   ```

5. **Open a new logon.** The token is built at logon, so a session that was already open does not see the change.
   Close the previous `runas` window and start a fresh one:

   ```
   runas /user:nrpoc cmd
   > whoami /priv
   SeCreatePermanentPrivilege    Create permanent shared objects      Disabled
   > C:\poc\crash_survival_permanent_job.exe
   ```

   The privilege shows as **Disabled**, which is expected: the examples enable it themselves with `set_privilege`
   (`AdjustTokenPrivileges`). The `Enabled` state in the output of the later sections is after that call.

6. **Cleanup** when done: remove `nrpoc` from the policy (same dialog, or drop its SID from the
   `SeCreatePermanentPrivilege` line and re-run `secedit /configure`), then `net user nrpoc /delete` and
   `rmdir /s /q C:\poc`. A leftover permanent job from a failed run can be removed by re-running the example (it calls
   `make_temporary`) or `list_jobs --no-demo --prefix nr-ac` to check that nothing remains. A reboot also clears them.

## Results

### `crash_survival_bookkeeping`: the child survives and is re-adopted

```
elevated: false
children of dead supervisor 8988: [10772] (child: 10772)
PASS
```

What happened:

1. The example re-executes itself as `supervisor`. It spawns `ping -n 300`, assigns it to an anonymous job, writes
   `<pid> <start_time> <supervisor_pid>` to a file and calls `std::process::abort()` (no destructors, the OS closes
   the handles as in a real crash).
2. The orchestrator (the "restarted AC") only knows the bookkept values. `adopt(pid, start_time + 1)` returns `None`
   (PID reuse protection), `adopt(pid, start_time)` returns the live process.
3. The dead supervisor shows up as the parent pid of the orphan in a process snapshot, no process needs to be opened
   for that.
4. `TerminateProcess` on the adopted handle works and the process exits.

### `crash_survival_permanent_job`: the job is the record

Same crash scenario, but nothing is bookkept. The supervisor enables `SeCreatePermanentPrivilege`, creates
`\BaseNamedObjects\nr-ac-crash-survival-<pid>` as a permanent job with `D:P(A;;GA;;;SY)(A;;GA;;;<user>)`, assigns
`ping -n 300` to it and calls `std::process::abort()`. The orchestrator only knows the name. As `SYSTEM`:

```
elevated: true
SeCreatePermanentPrivilege held: true
supervisor 7732: child 1056 in \BaseNamedObjects\nr-ac-crash-survival-2920
NamedJob { path: "\\BaseNamedObjects\\nr-ac-crash-survival-2920", name: "nr-ac-crash-survival-2920", permanent: true, other_handles: 0, active_processes: 1, pids: [1056] }
owner: "S-1-5-32-544", sddl: O:BAD:P(A;;0x1f003f;;;SY)
members: [1056]
cleanup: Ok(())
PASS
```

What happened:

1. After the crash the job is still listed (`list_named_jobs`): `permanent: true`, `other_handles: 0` and one active
   member. That combination is the signature of an orphan left by a crash.
2. The job is reopened by name (`Job::open_native`) and checked before trusting it: the owner is
   `BUILTIN\Administrators` (expected when the creator is `SYSTEM`) and `unexpected_grants` is empty.
3. The member comes from the job's process id list and is opened with `AdoptedProcess::open`. No `start_time` is
   involved, the job membership is the identity.
4. `TerminateProcess` works, the job reports 0 active processes and `make_temporary` removes the name. A `list_jobs
   --no-demo --prefix nr-ac` run afterwards finds nothing, and no `ping.exe` is left.

As the non-elevated user the example stops at the first step, as `permanent_job_probe` predicts:

```
elevated: false
SeCreatePermanentPrivilege held: false
FAIL: SeCreatePermanentPrivilege is required, run as SYSTEM or grant the right
```

As the standard user `nrpoc` with the right granted (`runas` logon, `Users` group only), it passes without elevation.
The owner is the user itself and the DACL contains only `SYSTEM` and that user:

```
elevated: false
SeCreatePermanentPrivilege held: true
supervisor 392: child 8156 in \BaseNamedObjects\nr-ac-crash-survival-8328
NamedJob { path: "\\BaseNamedObjects\\nr-ac-crash-survival-8328", name: "nr-ac-crash-survival-8328", permanent: true, other_handles: 0, active_processes: 1, pids: [8156] }
owner: "S-1-5-21-...-1002", sddl: O:S-1-5-21-...-1002D:P(A;;0x1f003f;;;SY)(A;;0x1f003f;;;S-1-5-21-...-1002)
members: [8156]
cleanup: Ok(())
PASS
```

### `job_name_probe`: a named job does not outlive its last handle

```
--- name: Local\nr-ac-probe-5908
assign: Ok(())
[handle held]
  via owned handle: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [2376], name: Some("\\Sessions\\1\\BaseNamedObjects\\nr-ac-probe-5908") })
  via open by name: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [2376], name: Some("\\Sessions\\1\\BaseNamedObjects\\nr-ac-probe-5908") })
[last handle closed]
  child in some job: true (Ok(()))
  via open by name: open failed: opening named job: The system cannot find the file specified. (0x80070002)
```

### `env_tag_probe`: tag discovery works, with a scan cost

```
elevated: Ok(false)
NR_AC_TAG env: Ok(Some("demo-tag"))
[full scan, no filter]
  inspected: 54, skipped (unreadable): 116, matches: [6364], took 13.480125ms
[recovery by tag, image filter ["ping.exe"]]
  inspected: 1, skipped (unreadable): 0, filtered out unopened: 169, took 3.477833ms
  matches: [6364] (original child: 6364)
  pid 6364: alive=Ok(true), start_time=Ok(134377622055093438)
  kill: Ok(())
  exited within 5s: true, alive=Ok(false)
```

### Same examples as `LocalSystem`

`crash_survival_bookkeeping` (`elevated: true`):

```
elevated: true
children of dead supervisor 6408: [7752] (child: 7752)
PASS
```

`job_name_probe`. The name now resolves in the session 0 namespace (`\BaseNamedObjects`, not
`\Sessions\1\BaseNamedObjects`), and it still disappears with the last handle:

```
--- name: Local\nr-ac-probe-2260
assign: Ok(())
[handle held]
  via owned handle: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [5008], name: Some("\\BaseNamedObjects\\nr-ac-probe-2260") })
  via open by name: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [5008], name: Some("\\BaseNamedObjects\\nr-ac-probe-2260") })
[last handle closed]
  child in some job: true (Ok(()))
  via open by name: open failed: opening named job: The system cannot find the file specified. (0x80070002)
```

`env_tag_probe`. SYSTEM can read far more processes (149 inspected, 19 unreadable, versus 54 and 116 as a
non-elevated user):

```
elevated: Ok(true)
NR_AC_TAG env: Ok(Some("demo-tag"))
[full scan, no filter]
  inspected: 149, skipped (unreadable): 19, matches: [3428], took 23.788709ms
[recovery by tag, image filter ["ping.exe"]]
  inspected: 1, skipped (unreadable): 0, filtered out unopened: 168, took 3.896792ms
  matches: [3428] (original child: 3428)
  pid 3428: alive=Ok(true), start_time=Ok(134377624949961667)
  kill: Ok(())
  exited within 5s: true, alive=Ok(false)
```

### Unit tests

```
running 3 tests
test tests::named_job_is_listed_with_its_attributes ... ok
test tests::mismatched_start_time_is_not_adopted ... ok
test tests::child_survives_job_handle_close_and_can_be_adopted ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
```

## Is it valid for non-admin users?

**Yes, for processes of the same user.** Every operation the approach needs is available at Medium integrity with a
non-elevated token (all runs above had `elevated: false`):

| Operation | API | Requirement |
|---|---|---|
| Create the job | `CreateJobObjectW(NULL, NULL)` | None |
| Put the child in the job | `AssignProcessToJobObject` | Rights on the child, which AC has because it spawned it |
| Re-open the sub-agent after a crash | `OpenProcess(SYNCHRONIZE \| QUERY_LIMITED_INFORMATION \| TERMINATE)` | Same user (or a higher privilege) |
| Start time / liveness / wait / kill | `GetProcessTimes`, `GetExitCodeProcess`, `WaitForSingleObject`, `TerminateProcess` | Those same rights |
| Parent pid lookup | `CreateToolhelp32Snapshot` | None, nothing is opened |
| Env tag scan (optional) | `OpenProcess(... \| PROCESS_VM_READ)` + `ReadProcessMemory` | Same user (or `SeDebugPrivilege`) |

Things that **do not** need admin, and are the reason the design avoids them: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
is simply not set, and the default strategy uses no object names and creates no permanent kernel objects (the
latter needs `SeCreatePermanentPrivilege`, see [permanent jobs](#making-the-name-survive-permanent-jobs)).

Limits of the claim: it holds as long as AC and the sub-agents run as the **same user**. A non-admin AC cannot open,
wait on or kill processes of another user or of `SYSTEM`.

## Named Job Objects: why a plain name is not enough

A named job looks like the obvious way to find the sub-agent again (`OpenJobObjectW(name)` and enumerate its pids).
The `job_name_probe` output shows why it does not work:

1. **The name only lives while a handle is open.** Names are entries in the object manager namespace and the entry is
   removed when the handle count reaches zero. After a crash the OS closes AC's handle, and since AC held the only
   one, the name is gone. In the output, `open by name` works while the handle is held and fails with
   `ERROR_FILE_NOT_FOUND (0x80070002)` right after `drop(job)`.
2. **The job itself is still there.** `IsProcessInJob` reports `child in some job: true`, so the child is still a
   member of an unreachable job. There is no API to get a handle to a job from its member processes or by pid, only
   from inside the process (`QueryInformationJobObject(NULL, ...)`, which `Job::current_info` uses).
3. **The only way to keep the name is a permanent object**, which needs `SeCreatePermanentPrivilege`. It works as
   `SYSTEM` but not as a non-elevated user, see [Making the name survive](#making-the-name-survive-permanent-jobs).
   Having another process hold a handle just to pin the name would be a new thing to supervise with the same crash
   problem.
4. **Names are not a safe identity anyway.**
   - `CreateJobObjectW` on an existing name returns a handle to the *existing* job (with `ERROR_ALREADY_EXISTS`), so
     any process that guesses the name first owns it. `permanent_job_probe` shows a non-elevated user can create
     jobs in the global `\BaseNamedObjects` directory, so squatting a predictable name is possible.
   - `Local\` names are per terminal session: the same `Local\` name resolved to `\Sessions\1\BaseNamedObjects\...`
     for the interactive user and to `\BaseNamedObjects\...` for SYSTEM in session 0 (the AC service), so a name
     opened from another session does not resolve unless `Global\` is used.

So a plain (temporary) name does not help: the **persisted/observable identity** is what survives a crash. The
exception is a permanent named job, which does survive but has the trade-offs listed below.

### Making the name survive: permanent jobs

Kernel objects can be created with the `OBJ_PERMANENT` attribute, which adds one extra reference owned by the object
manager. The name then stays in the namespace after the last handle closes, until something calls
`NtMakeTemporaryObject` on it (or the machine reboots). This is the only mechanism that makes a job name survive the
loss of its handles. There is no Win32 flag for it: `CreateJobObjectW` cannot do it, so the PoC calls the native
`NtCreateJobObject` with `OBJ_PERMANENT` (`permanent_job_probe`).

The required privilege is `SeCreatePermanentPrivilege` ("Create permanent shared objects"). Whether it is held is
visible in `whoami /priv`:

| Identity | `SeCreatePermanentPrivilege` in token | `NtCreateJobObject(OBJ_PERMANENT)` |
|---|---|---|
| Non-elevated user (`prlctl --current-user`) | Not present | `0xC0000061` `STATUS_PRIVILEGE_NOT_HELD`, in both `\BaseNamedObjects` and `\Sessions\1\BaseNamedObjects` |
| `SYSTEM` (what `install.ps1` creates) | Present, enabled | Succeeds. After the last handle closes, `open by name` still returns the job with its member pid |
| Standard user `nrpoc` **after an admin granted the right** | Present but **disabled** in the token, enabled by the probe with `AdjustTokenPrivileges` (default `enable` mode) | Succeeds in both namespaces, same behavior as `SYSTEM` |

The same code path with `OBJ_PERMANENT` cleared (baseline) succeeds for both identities, so the failure is the
privilege and not the object directory permissions.

`permanent_job_probe` output as the non-elevated user:

```
elevated: Ok(false), session: 1
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
--- session namespace: \Sessions\1\BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
```

And as `SYSTEM` (`prlctl exec` without `--current-user`):

```
elevated: Ok(true), session: 0
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [2448], name: Some("\\BaseNamedObjects\\nr-ac-perm-probe") })
  permanent: made temporary, NTSTATUS 0x00000000
  permanent: after cleanup, open by name: opening named job: The system cannot find the file specified. (0x80070002)
```

A privilege that is **held but disabled** does not work. `SYSTEM` has `SeCreatePermanentPrivilege` enabled in its
token, so it can be disabled with `AdjustTokenPrivileges` to test that case without any policy change
(`permanent_job_probe -- <enable|disable|leave>`, `set_privilege` in `src/lib.rs`). Run as `SYSTEM`:

```
##### mode=leave   (token as issued)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, ... name: Some("\\BaseNamedObjects\\nr-ac-perm-probe") })
##### mode=disable (held, disabled)
SeCreatePermanentPrivilege held, after disable: Ok(true)
  permanent: create failed, NTSTATUS 0xC0000061
##### mode=enable
SeCreatePermanentPrivilege held, after enable: Ok(true)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, ... name: Some("\\BaseNamedObjects\\nr-ac-perm-probe") })
```

So the privilege must be present **and enabled** in the token. This matters for a non-`SYSTEM` account: `whoami /priv`
for `nrpoc` (in a new `runas` logon) shows the right is granted **disabled**, so the process has to enable it itself
at startup.

`nrpoc` before granting the right (no `SeCreatePermanentPrivilege`):

```
Privilege Name                Description                          State
============================= ==================================== ========
SeShutdownPrivilege           Shut down the system                 Disabled
SeChangeNotifyPrivilege       Bypass traverse checking             Enabled
SeUndockPrivilege             Remove computer from docking station Disabled
SeIncreaseWorkingSetPrivilege Increase a process working set       Disabled
SeTimeZonePrivilege           Change the time zone                 Disabled
```

`nrpoc` after granting it (the other rows are unchanged):

```
Privilege Name                Description                          State
============================= ==================================== ========
SeCreatePermanentPrivilege    Create permanent shared objects      Enabled
...
```

Running the probe as `nrpoc` with the right granted but **without enabling it** (`permanent_job_probe -- leave`)
fails, as it did for `SYSTEM` with the privilege disabled:

```
elevated: Ok(false), session: 1
SeCreatePermanentPrivilege: token left as issued
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
--- session namespace: \Sessions\1\BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: create failed, NTSTATUS 0xC0000061
```

#### Non-admin user with the right granted

To check whether a non-admin AC could use this, a local standard user (`nrpoc`, `Users` group only) ran the probe
from a new logon (`runas /user:nrpoc cmd`) before and after an administrator added it to **Create permanent shared
objects** in `secpol.msc`.

Before granting the right, the result is the same as for the non-elevated `calvarez` session above
(`held and enabled: Ok(false)`, `0xC0000061` in both namespaces).

After granting it:

```
C:\poc\permanent_job_probe.exe
elevated: Ok(false), session: 1
SeCreatePermanentPrivilege held and enabled: Ok(true)
--- global namespace: \BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [5536], name: Some("\\BaseNamedObjects\\nr-ac-perm-probe") })
  permanent: made temporary, NTSTATUS 0x00000000
  permanent: after cleanup, open by name: opening named job: The system cannot find the file specified. (0x80070002)
--- session namespace: \Sessions\1\BaseNamedObjects\nr-ac-perm-probe
  temporary: created, assign: Ok(())
  temporary: last handle closed, open by name: opening named job: The system cannot find the file specified. (0x80070002)
  permanent: created, assign: Ok(())
  permanent: last handle closed, open by name: Ok(JobInfo { active_processes: 1, total_processes: 1, pids: [10180], name: Some("\\Sessions\\1\\BaseNamedObjects\\nr-ac-perm-probe") })
  permanent: made temporary, NTSTATUS 0x00000000
  permanent: after cleanup, open by name: opening named job: The system cannot find the file specified. (0x80070002)
```

So **yes, the name can survive handle loss, and it does not require running as `SYSTEM` or elevated**: a non-elevated
standard user that holds `SeCreatePermanentPrivilege` can create a permanent named job in both the global and the
session namespace and clean it up. The condition is that an **administrator grants the user right once**, at install
time (Local Security Policy, `secedit` or `LsaAddAccountRights`). There is no runtime elevation, but the grant is a
security-sensitive policy change: the right allows creating permanent shared objects in the object manager, so it
should be given to a dedicated service account and not to all users. The token also needs a new logon to pick the right
up.

What it would buy and cost for AC (`LocalSystem`, or a non-admin account that was granted the right):

| Pros | Cons |
|---|---|
| Membership is authoritative: `JobObjectBasicProcessIdList` returns the live members, descendants included, with no PID reuse ambiguity and no `(pid, start_time)` store | A non-admin AC needs an administrator to grant `SeCreatePermanentPrivilege` once at install time, and must enable it in its token at startup |
| No persisted state to keep in sync, no spawn-to-write window | Lifecycle is manual: the name leaks until reboot unless AC calls `NtMakeTemporaryObject` on sub-agent removal. The handle needs `DELETE` access, which `OpenJobObjectW(JOB_OBJECT_QUERY)` does not request |
| One job per sub-agent also gives AC a place to attach limits later | Relies on the native API (`NtCreateJobObject`, `NtMakeTemporaryObject`), not the documented Win32 surface |
| | Name squatting: a regular user can create a temporary job under the same name first (shown above for `\BaseNamedObjects`), so a name must be unguessable or the creator must be verified (`STATUS_OBJECT_NAME_EXISTS` handling and DACL checks) |
| | A permanent job with no members stays around until cleaned, so restarts need an "open or create" path that tolerates leftovers |

The DACL of a permanent job is covered in [DACL of a permanent job](#dacl-of-a-permanent-job). Name squatting and the
leftover behavior are not exercised by the probes, see [Future work](#future-work).

### DACL of a permanent job

A permanent job outlives its creator, so whoever can open it by name matters. `job_dacl_probe` creates one permanent
job with the default security descriptor and one with an explicit SDDL, reads owner and DACL from the handle
(`GetSecurityInfo` with `SE_KERNEL_OBJECT`, ACEs walked with `GetAce`, SDDL from
`ConvertSecurityDescriptorToStringSecurityDescriptorW`), closes the handle, reopens the job by name with `READ_CONTROL`
and reads it again. It finishes with `make_temporary`.

The default DACL comes from the creator's token. It is not a fixed value, and it differs per identity:

| Creator | Owner | Default DACL (SDDL) |
|---|---|---|
| `SYSTEM` | `BUILTIN\Administrators` | `O:BAD:(A;;0x1f003f;;;SY)(A;;0x120004;;;BA)` |
| Non-elevated `calvarez`, `\BaseNamedObjects` | `calvarez` | `...(A;;0x1f003f;;;<user>)(A;;0x1f003f;;;SY)(A;;0x120004;;;S-1-5-5-0-<logon session>)` |
| Non-elevated `calvarez`, `\Sessions\1\BaseNamedObjects` | `calvarez` | `...(A;;0x1f003f;;;SY)(A;;0x1f003f;;;<user>)(A;;0x1f003f;;;S-1-5-5-0-<logon session>)` |

So the default is not "open to everyone", but it is not minimal either:

- Under `SYSTEM`, **`Administrators` get `0x120004`**: `READ_CONTROL | SYNCHRONIZE | JOB_OBJECT_QUERY`. That lets any
  administrator open the job, list its members and read the DACL, but not terminate it or change its limits.
  `JOB_OBJECT_QUERY` is the right `Job::open` needs, so an admin AC can reopen the job.
- Under a non-elevated user, the **logon session SID** (`S-1-5-5-0-*`) is also granted access. In the session namespace
  it is a full `0x1f003f` grant to every process in the same logon session, which includes processes of that user that
  AC may not trust (and, if the account is shared, other users of that session).
- `0x1f003f` is `JOB_OBJECT_ALL_ACCESS` (`GA` expands to it). Nothing here comes from the object directory: no ACE is
  inherited, all of it is the token default DACL.
- The DACL is stored in the object. The reopened job showed exactly the same SDDL after the last handle was closed.

The permanent create still fails with `0xC0000061` for a user without the privilege, so for that identity the probe
repeats the same steps on a temporary job. Creation with a security descriptor behaves the same in both cases.

#### Creating the job with a restrictive security descriptor

Pass the descriptor in `OBJECT_ATTRIBUTES.SecurityDescriptor` to `NtCreateJobObject`, so the job never exists with the
default DACL (a `SetSecurityInfo` afterwards would leave a window). The PoC builds it from an SDDL string with
`ConvertStringSecurityDescriptorToSecurityDescriptorW` (`Job::create_native_with_sddl`):

```
D:P(A;;GA;;;SY)(A;;GA;;;<AC user SID>)
```

| Part | Meaning |
|---|---|
| `D:` | DACL only. Owner and group are left to the token default |
| `P` | Protected: the DACL does not take inherited ACEs |
| `(A;;GA;;;SY)` | Allow `GENERIC_ALL` to `SYSTEM`. The object manager maps `GA` to the job's `0x1f003f` |
| `(A;;GA;;;<user SID>)` | Allow `GENERIC_ALL` to the identity AC runs as (`current_user_sid`, read from the token). Omitted when AC is `SYSTEM`, where it would be a duplicate |

Output as `SYSTEM` (permanent, handle closed, reopened by name):

```
  [restrictive DACL]
    requested SDDL: D:P(A;;GA;;;SY)
    after creation (permanent: true), read from the handle:
      owner: S-1-5-32-544 (BUILTIN\Administrators)
      SDDL:  O:BAD:P(A;;0x1f003f;;;SY)
      ACE:   Allow 0x001F003F S-1-5-18 (NT AUTHORITY\SYSTEM)
      audit: only the current user and SYSTEM are granted access
    last handle closed, reopened by name with READ_CONTROL:
      SDDL:  O:BAD:P(A;;0x1f003f;;;SY)
    cleanup: Ok(())
```

And as the non-elevated user (temporary job, permanent is refused), the logon session ACE is gone:

```
      SDDL:  O:S-1-5-21-...-1000D:P(A;;0x1f003f;;;SY)(A;;0x1f003f;;;S-1-5-21-...-1000)
      audit: only the current user and SYSTEM are granted access
```

Notes for using it:

- **Check after creation.** Two checks, both printed by `job_dacl_probe` as `audit:` lines:
  - `ObjectSecurity::unexpected_grants` returns every allow ACE (or a NULL DACL) whose trustee is not in an expected
    list.
  - `ObjectSecurity::unexpected_owner` returns the owner when it is not an expected SID. The owner is the identity-based
    check: only the creator can set it, so a squatter's job shows its own account. Expect the AC user for a non-admin
    AC, and `BUILTIN\Administrators` (`S-1-5-32-544`) when the creator is `SYSTEM` or an elevated token, as seen in
    the `SYSTEM` run.

  AC can run them right after creating the job, or on reopening one, and refuse to adopt a job that fails either. The
  owner check passed in all runs (`audit: owner is the current user or Administrators`). A unit test also checks that
  an unrelated SID is rejected.
- **Keep `DELETE` for the cleaner.** `make_temporary` needs `DELETE` on the handle, so the identity that removes the
  job must be granted it. `GA` covers it. A read-only ACE (for example `(A;;0x120004;;;BA)` to let administrators
  inspect the job) can be added without allowing termination.
- **Administrators are not locked out for real.** Without an ACE they cannot open the job, but a holder of
  `SeTakeOwnershipPrivilege` can take ownership and rewrite the DACL. The DACL protects against other accounts and
  processes, not against a local administrator.
- **Different identity.** Per the object security model, an identity without a matching allow ACE gets
  `ERROR_ACCESS_DENIED` on open. This was not tried with a second account, as the DACL is documented behavior.
- **An upgrade changing the AC account** (for example `SYSTEM` to a dedicated user) would find old jobs it cannot open.
  The restrictive SDDL above would need the old SID too during the transition.

#### Is a restrictive DACL worth it for AC?

**Short answer: it is cheap hardening, not a security boundary. Set it explicitly, but do not rely on it, and put the
effort in verifying owner and DACL when adopting a job.**

What a wider DACL would allow, and who could abuse it:

| AC runs as | Default DACL already grants | Extra exposure vs the restrictive SDDL | Worth it? |
|---|---|---|---|
| `SYSTEM` | `SYSTEM` full, `Administrators` `0x120004` (query, read DACL) | Administrators can inspect the job. They can already kill any process and take ownership of the job, so there is nothing to protect them from | **Marginal.** The default is already what we would write by hand (`D:P(A;;GA;;;SY)(A;;0x120004;;;BA)` is equivalent). Setting it explicitly only pins the result, so it no longer depends on the token default DACL |
| Non-admin user | User and `SYSTEM` full, plus the **logon session SID** full | Every process in the user's logon session can terminate the job, set limits or make it temporary. That includes the sub-agents themselves, which are children of AC | **Modest.** Same-user processes can already `TerminateProcess` each other, so the job is not the weak point. What it removes is the job-level controls (limits, `NtMakeTemporaryObject`) from a compromised sub-agent, which is a real but small gain |

Why it is not worth more:

- **Same user is already inside the trust boundary.** This PoC is scoped to same-user adoption, and a same-user process
  can already open and kill the sub-agent. The DACL does not change that.
- **Local administrators can bypass it** (take ownership, rewrite the DACL), see the notes above.
- **It does not stop name squatting.** A squatter creates its own job under the same name with its own DACL. The check
  that matters is on the *existing* job: its **owner** must be the expected SID (`ObjectSecurity::owner`) and
  `unexpected_grants` must be empty. A DACL on AC's job is only meaningful if AC verifies it on reopen.
- **It adds failure modes.** An admin cannot inspect the job when `BA` is not granted. Changing the AC account (for
  example `SYSTEM` to a dedicated user, or the reverse) leaves jobs the new identity cannot open, and the cleanup
  (`NtMakeTemporaryObject` needs `DELETE`) would fail too.

Recommendation:

1. Create the job with an explicit SDDL in both modes (the code path is the same, `create_native_with_sddl`). Use
   `SYSTEM` plus the AC identity, and add `(A;;0x120004;;;BA)` for the `SYSTEM` case to keep it supportable.
2. Always run the audit and the owner check when reopening a job after a restart, and treat a mismatch as "not our
   job" (do not adopt, do not kill).
3. Do not count the DACL as protection against the sub-agent or other same-user processes. If that matters, it needs a
   different identity for the sub-agents, not a tighter job ACL.

### Listing and filtering named jobs

A restarted AC can enumerate named jobs instead of relying on a persisted list. `list_jobs` does it with
`NtOpenDirectoryObject` + `NtQueryDirectoryObject` on `\BaseNamedObjects` (and on `\Sessions\<n>\BaseNamedObjects`
when not in session 0):

1. Each directory entry has a name and a type name. Only entries of type `Job` are kept.
2. Each job is opened with `NtOpenJobObject(JOB_OBJECT_QUERY)` and queried. `NtQueryObject(ObjectBasicInformation)`
   gives `Attributes` (`OBJ_PERMANENT` means permanent) and `HandleCount`, and the job queries give the members. The
   inspecting handle is counted by the kernel, so `other_handles = HandleCount - 1`.
3. Filtering is plain Rust on the result: name prefix and permanent flag. Permanent with `other_handles == 0` is a job
   whose name is only alive because it is permanent, which is the leftover after an AC crash.

The example creates three demo jobs in `\BaseNamedObjects` (a permanent orphan with a running member and no handles,
a temporary one kept alive by the example, and a decoy outside the prefix), lists, filters, and then makes the
permanent job temporary again. As `SYSTEM`:

```
elevated: Ok(true), session: 0
SeCreatePermanentPrivilege held: Ok(true)
--- \BaseNamedObjects
[all] jobs: 5, unreadable jobs: 0, entries of other types: 741
  \BaseNamedObjects\WmiProviderSubSystemHostJob permanent=false other_handles=1 active_processes=0 pids=[]
  \BaseNamedObjects\WmiProviderSubSystemSpecialHostJob permanent=false other_handles=1 active_processes=0 pids=[]
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[5636]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  \BaseNamedObjects\other-list-demo-decoy permanent=false other_handles=1 active_processes=0 pids=[]
[filter: name starts with "nr-ac-list-demo-", permanent only: false]
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[5636]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  matches: 2
  permanent jobs nobody holds a handle to: 1
cleanup: make_temporary(\BaseNamedObjects\nr-ac-list-demo-permanent-orphan): Ok(())
```

With `--permanent` only the orphan matches (`matches: 1`). A run with `--no-demo --prefix nr-ac` afterwards finds no
`nr-ac*` job, so nothing is left behind.

As the non-elevated user the permanent demo job cannot be created (`held: Ok(false)`), and **2 of the jobs in
`\BaseNamedObjects` are unreadable**: the two WMI jobs listed above are not openable for query, so a non-admin sees
only the jobs its DACL allows. The jobs it created itself are listed normally:

```
elevated: Ok(false), session: 1
SeCreatePermanentPrivilege held: Ok(false)
--- \BaseNamedObjects
[all] jobs: 2, unreadable jobs: 2, entries of other types: 731
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  \BaseNamedObjects\other-list-demo-decoy permanent=false other_handles=1 active_processes=0 pids=[]
[filter: name starts with "nr-ac-list-demo-", permanent only: false]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  matches: 1
  permanent jobs nobody holds a handle to: 0
--- \Sessions\1\BaseNamedObjects
[all] jobs: 0, unreadable jobs: 0, entries of other types: 680
  matches: 0
```

As the standard user `nrpoc` with the right granted, it behaves like `SYSTEM` for its own jobs: the privilege is
enabled by `set_privilege`, the permanent orphan is created, found by the filters and made temporary again. The two
WMI jobs are unreadable, as for the other non-elevated user. The `\Sessions\1` directory (the `runas` logon is in
session 1) has no jobs:

```
elevated: Ok(false), session: 1
SeCreatePermanentPrivilege held: Ok(true)
--- \BaseNamedObjects
[all] jobs: 3, unreadable jobs: 2, entries of other types: 730
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[8512]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  \BaseNamedObjects\other-list-demo-decoy permanent=false other_handles=1 active_processes=0 pids=[]
[filter: name starts with "nr-ac-list-demo-", permanent only: false]
  \BaseNamedObjects\nr-ac-list-demo-permanent-orphan permanent=true other_handles=0 active_processes=1 pids=[8512]
  \BaseNamedObjects\nr-ac-list-demo-temporary-held permanent=false other_handles=1 active_processes=0 pids=[]
  matches: 2
  permanent jobs nobody holds a handle to: 1
--- \Sessions\1\BaseNamedObjects
[all] jobs: 0, unreadable jobs: 0, entries of other types: 647
  matches: 0
cleanup: make_temporary(\BaseNamedObjects\nr-ac-list-demo-permanent-orphan): Ok(())
```

Not covered: whether `nrpoc` can read a permanent job created by `SYSTEM` (or the reverse), which depends on the default
DACL, see [Future work](#future-work).

Limits: only named jobs are listed, a temporary job shows up only while some process holds a handle, and the filter
by name prefix is not an identity check (see name squatting in [Future work](#future-work)).

## Discovery strategies

Once the job is out of the picture, the restarted AC needs some way to know *which* process is its sub-agent.

### Bookkeeping: persist `(pid, start_time)` at spawn

`crash_survival_bookkeeping` uses this. `adopt` opens the pid, checks it is alive and that its creation time equals the
persisted one.

| Pros | Cons |
|---|---|
| O(1): one `OpenProcess`, no enumeration | Stateful: needs a store that is written at spawn and kept in sync |
| Only needs the rights to open one process of the same user | Window between `CreateProcess` and the write: a crash in there leaves an unrecorded orphan |
| Exact: `start_time` rules out PID reuse | Stale or corrupt state must be handled (process exited, pid reused, file lost) |
| No cross-process memory reads, nothing for EDR to flag | Does not see grandchildren that outlive the sub-agent unless they are also recorded |
| Works for any sub-agent binary, no cooperation needed | The store needs a location AC can write, with the right ACLs |

The parent pid kept by Windows after the parent exits (`find_processes_by_parent`) is a cheap complement: it narrows
candidates for an unrecorded orphan without opening anything. It is a hint, not an identity, because the pid can be
reused.

### Environment variable tag: set `NR_AC_TAG=<id>` at spawn and scan

`env_tag_probe` uses this. The tag is read from the target's environment block through `NtQueryInformationProcess` +
`ReadProcessMemory`.

| Pros | Cons |
|---|---|
| Stateless: nothing to persist, no window between spawn and write | Reads the remote PEB with **hardcoded x64 offsets** (`0x20`, `0x80`, `0x3F0`), undocumented and layout dependent (32-bit/WoW64 targets need other offsets) |
| Finds the orphan even if AC lost all its state | Needs `PROCESS_VM_READ`: limited to same-user processes, and `ReadProcessMemory` scans are what EDR products look for |
| Descendants inherit the tag, so grandchildren can be found too | Descendants inheriting the tag also means false positives if the tag is not unique per sub-agent instance |
| | Cost grows with the number of processes. Without a filter 170 processes were visited and 116 were unreadable; with an image name filter only 1 was opened (3.5 ms vs 13.5 ms) |
| | Only the start-up environment is visible, and the sub-agent can scrub or change it |
| | Spoofable: any process of the same user can set the same variable. An image name check helps, but it is not an identity |
| | Still needs `start_time`/image checks to be safe against PID reuse once the match is found |

### Comparison by user

AC currently installs as a Windows service with `New-Service ... -StartupType Automatic` and **no `-Credential`**
([`install.ps1`](../build/package/windows/install.ps1)), so it runs as **`LocalSystem`**. The install directory and
the data directory are locked to `BUILTIN\Administrators` with `icacls` (`Set-RestrictedAcl`). The alternative is a
non-admin user running AC.

| | **`LocalSystem` (current `install.ps1`)** | **Non-admin user** |
|---|---|---|
| Job + no kill-on-close | Works (verified) | Works (verified) |
| Bookkeeping | Works (verified, `crash_survival_bookkeeping` PASS). State lives under the admin-only `ProgramData`/`Program Files` dirs, which is a good home for it. SYSTEM can open any process, so adoption does not depend on who spawned it | Works (verified). Only same-user processes can be adopted. The store must be in a location that user can write, so it cannot reuse the admin-only ACLs |
| Env tag scan | Works (verified). SYSTEM could read 149 of 168 other processes versus 54 of 170 for the non-admin user, so the scan reaches **more** processes, and the no-filter case is slower (23.8 ms vs 13.5 ms). The image filter keeps it at 1 opened process (3.9 ms). Protected processes (PPL) stay unreadable | Works for own processes (verified). Everything else is skipped (116 of 170 in our run), which keeps the match set naturally scoped to this user |
| Spoofing of the tag | A user process that copies the tag can make SYSTEM adopt (and later kill) a process that is not a sub-agent. The impact is bounded by the image name/path/start time checks, but the trust boundary is crossed | Only same-user processes are scanned, so a spoofer already has the same rights as AC |
| Blast radius of a wrong adoption | SYSTEM can terminate nearly anything | Limited to the user's own processes |
| Installation | Needs an admin (already required) | A service still needs an admin to be registered; a fully non-admin deployment needs another launcher (e.g. a per-user task) |

Summary:

- **Bookkeeping** behaves the same for both users and needs the fewest privileges and no native APIs, but it is not
  the more robust option. The record can be lost or corrupted. A corrupted record fails safe (a wrong `(pid,
  start_time)` is rejected, nothing unrelated is killed), but a lost one fails **open and silently**: the orphan keeps
  running and the restarted AC starts a second copy of the sub-agent. Mitigations are an atomic write (temp file and
  rename), persisting before the child can do anything else, and the fallbacks below.
- **Env tag** is attractive for being stateless, but it depends on undocumented structure offsets and on reading other
  processes' memory. Under `LocalSystem` it widens the trust boundary (scan scope and spoofing), while for a non-admin
  user it is naturally scoped to the user's own processes.
- A **permanent named job** is the only option where the OS holds the record. With the owner and DACL checks
  ([DACL of a permanent job](#dacl-of-a-permanent-job)) the membership is authoritative and verifiable, there is no
  store to lose or corrupt, and a failure is loud (`ERROR_ACCESS_DENIED`, owner mismatch) instead of a silent duplicate.
  Its costs are a one-time `SeCreatePermanentPrivilege` grant for a non-admin AC (free under `SYSTEM`), manual cleanup
  and leftover handling, native APIs, and an owner check that cannot tell `SYSTEM` from another administrator.
- The trade-off is therefore **robustness against lost state (permanent job) versus privilege and simplicity
  (bookkeeping)**, and it depends on who AC runs as:
  - `SYSTEM`: the privilege is already there, so the permanent job costs little and removes the lost-state failure.
  - Non-admin: it needs an administrator to grant the right at install time. Where that is not acceptable, bookkeeping
    with the parent pid (and optionally the tag with an image filter) as fallbacks is the practical choice.
  - Supporting both (permanent job when the privilege is held, bookkeeping otherwise) covers each case, at the price of
    two code paths to test.
- Either way, spawn the child **suspended**, record it (persist, or assign it to the job), and only then resume it.
  That closes the spawn-to-record window for both approaches. The PoC does not do this yet: it assigns the child right
  after `spawn`.

`crash_survival_permanent_job` exercises the permanent job end to end, including the cleanup, as `SYSTEM` and as a
standard user with the right granted. Not verified: repeated restarts and leftovers across sub-agent removal (see
[Future work](#future-work)), so the permanent job path is still less proven than bookkeeping.

## Limitations

- **Most runs use a filtered admin account, not a standard user.** `calvarez` is in the Administrators group, but the
  session runs with a filtered, non-elevated token. This proves no *elevation* is needed. Only `permanent_job_probe`,
  `list_jobs` and `crash_survival_permanent_job` were also run under a dedicated standard user (`nrpoc`); `crash_survival_bookkeeping` and `env_tag_probe` were not, so UAC
  virtualization or different default ACLs on the data directories are not exercised.
- **The `LocalSystem` runs are not an actual service.** They execute as SYSTEM through Parallels Tools, in session 0,
  which matches the identity and session of the AC service but not its service control manager lifecycle. The
  spoofing and blast radius rows of the comparison table are analysis, not something the examples exercise.
- **Same-user only.** Cross-user adoption is out of scope.
- The env scan hardcodes 64-bit PEB offsets and is only meant to measure feasibility and cost.
- The sub-agent is a stand-in (`ping`). Real agents may spawn their own children, restart themselves or run inside
  a job already (nested jobs are allowed on Windows 8 and later, but breakaway rules were not probed).
- `AssignProcessToJobObject` happens right after spawn, so a very short-lived grandchild created in between would
  not be in the job.

## Future work

Open points for the permanent named job approach. None of them is covered by the current examples:

- **Cross-identity access to a permanent job.** The DACL is documented and checked after creation (see
  [DACL of a permanent job](#dacl-of-a-permanent-job)), but nobody tried to open a job created by `SYSTEM` as
  `calvarez` or `nrpoc`, or the reverse. `job_dacl_probe` was not run as `nrpoc` with the privilege granted either.
- **Name squatting.** A regular user can create a temporary job in `\BaseNamedObjects` under any name, so it could
  take the name before AC does. A DACL on AC's own job does not prevent that. Things to evaluate:
  - How `NtCreateJobObject` reports an existing name (`STATUS_OBJECT_NAME_EXISTS`) and whether AC can detect that it
    did not create the job.
  - Verifying the owner or DACL of an existing job before trusting it.
  - Using an unguessable name per sub-agent instance.
- **Leftover permanent jobs.** `list_jobs` shows how to find them. Still to cover: behavior across AC restarts and
  sub-agent removal, including the "open or create" path and calling `make_temporary` on cleanup, and reading jobs created by
  another account.
