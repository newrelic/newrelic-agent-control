# Spike: F-08 — Event loop frozen during on-host self-update

**Status:** investigation / proposal (no fix implemented)
**Finding:** F-08 — Event loop frozen ~21 s during upgrade; OpAMP status reporting stalls, Fleet Control marks the agent unhealthy (P1).
**Related:** F-06 (verify opens an OpAMP connection), F-07 (unnecessary sub-agent restarts), F-10 (lost intermediate configs), F-18 (concurrent update calls).

---

## 1. Current behavior and why it freezes

### Call sites

`VersionUpdater::update()` is invoked **synchronously** from two places, both on the main thread:

- **Startup / rollback** — `agent_control.rs:185-188`, before the event loop starts. Handles the "previous upgrade left the stored remote config pointing at the old version" rollback case.
- **Remote-config handling** — `agent_control.rs:468`, inside `validate_apply_store_remote_config()`, which is called from `handle_remote_config()`, which runs inside the **OpAMP `RemoteConfigReceived` arm** of the `select!` loop (`agent_control.rs:300-370`).

The event loop is a synchronous `crossbeam::select!` (`agent_control.rs:299-370`) running on the main thread, with four arms:

| Arm | Purpose |
|-----|---------|
| OpAMP events | `RemoteConfigReceived` → `handle_remote_config` → **`update()` (:468)** |
| AC internal events | `HealthUpdated → report_health`, `AgentControlAttributesUpdated`, `SelfUpdateRestartRequested → break` |
| Application events | SIGTERM/SIGINT → `break GracefulShutdownReason::ExternalRequested` |
| Uptime reporter tick | periodic uptime report |

### Blocking operations inside `update()` → `try_upgrade()` (`version_updater/on_host.rs`)

1. **`package_manager.install(...)`** — OCI download of the new binary (~20 MB), `runtime.block_on(...)` on the shared runtime. ~5–10 s.
2. **`verify_executor.execute(&new_binary_path, &["verify"])`** — spawns the new binary as a subprocess running `verify`; polls with a 2 s interval up to a 20 s timeout (`verify.rs:13-14`). The subprocess itself does config + **OpAMP connectivity** checks (~8 s per `verify.rs:10-12` — this is F-06).
3. **`BinarySelfReplacer::self_replace(...)`** then **publish `SelfUpdateRestartRequested`**.

While `update()` runs (~21 s), the `select!` loop cannot reach any `recv()`, so:

- **No remote-config processing** — further OpAMP configs queue.
- **No AC internal-event processing** — `HealthUpdated` events from the health-checker thread queue; `report_health()` (`:565`) is never called, so the agent's health/status is **not propagated to OpAMP** for the duration. This is the most likely driver of "FC reports unhealthy": with `poll_interval` of 5 s, 4+ reporting cycles carry stale/no fresh status.
- **No SIGTERM handling** — the application-event arm can't run; the process is unkillable-by-signal for ~21 s.

> Note on raw OpAMP heartbeats: the OpAMP managed client polls from its own task. Because the shared runtime is **multi-threaded** (`run.rs:92`), the low-level poll *may* keep firing on a worker thread even while the main thread is blocked in `block_on`. The user-visible "unhealthy" is therefore best explained by **health-status propagation stalling through the frozen loop** and/or the verify subprocess's own OpAMP connection (F-06). *Confirm the exact heartbeat path before finalizing acceptance criteria.*

### State ownership (constraints for any fix)

- `version_updater: VU` is owned by `AgentControl` (the `self` of the loop).
- `OnHostACUpdater` holds: `package_manager`, `verify_executor`, `repository`, `pub_key_url`, an `agent_control_internal_publisher` (clonable, `Send`), and a **`BackoffGate<Version, C>` that wraps `RefCell<GateState>` → it is `!Sync`**. The gate is the cooldown/dedup state keyed by target version.
- `retry()` + `SelfUpdateConfig::retry_heartbeat` exist but **are not wired into the loop** — there is no production `.retry()` caller. So today the only thing that re-drives a failed/pending upgrade is the next remote-config push hitting `:468`.

---

## 2. Implementation options

All options share one principle: **the heavy work (download + verify + self-replace) must leave the main thread**, and completion/failure must come back as an event the loop already knows how to handle (`SelfUpdateRestartRequested`, plus a new failure/▸status event).

### Option A — Background tokio task (review's suggested direction)

Spawn the upgrade work as a task on the existing shared runtime; on completion publish `SelfUpdateRestartRequested` (success) or a new `SelfUpdateFailed`/status event (failure) to the internal consumer.

- **Pros:** Reuses the existing runtime; download is already async (`block_on` today). Minimal new threading primitives.
- **Cons:** `verify` and `self_replace` are blocking/sync (subprocess polling, file ops) — must wrap in `spawn_blocking`. The `BackoffGate` is `!Sync` and can't be moved into the task as-is; gate state must stay main-thread-owned (see Option D). Cancellation on SIGTERM mid-download needs an abort path.

### Option B — Dedicated worker thread (`ThreadContext` precedent)

Run a single "self-update worker" via the existing `NotStartedThreadContext` / `spawn_named_thread` pattern (same precedent as `spawn_health_checker` and sub-agent runtimes). The main loop sends it "upgrade to version X" requests over a channel; it reports back via the internal publisher and a `CancellationMessage` stops it.

- **Pros:** Matches established codebase patterns (health checker, sub-agent threads); clean lifecycle/stop semantics; no async coloring of `verify`/`self_replace`. The worker can own the blocking OCI `block_on` without starving the runtime.
- **Cons:** One more long-lived thread; need a small protocol (request/result channel) and a single-flight guard.

### Option C — State machine driven by the existing loop (chunked, no extra thread)

Decompose `update()` into non-blocking steps and add a timer arm that advances an `UpdateState` enum (`Idle | Downloading | Verifying | Replacing`) a little each tick.

- **Pros:** No new thread/task; everything stays on the main loop; easiest to reason about ordering vs `apply_remote_config_agents`.
- **Cons:** The blocking ops (subprocess `verify`, OCI `block_on`) don't naturally chunk — you'd still block per step unless you also offload them, which defeats the purpose. **Not recommended** on its own.

### Option D — Recommended: worker thread (B) + main-loop-owned single-flight state machine

Keep the **decision state on the main thread** and offload only the **execution**:

- Main loop owns the `BackoffGate` + a small `UpdateController` with `Idle | InFlight { version } | …` and a `desired_version: Option<Version>` (latest target seen).
- On `update(new_config)` at `:468`: validate/dedup via the gate as today, but instead of running `try_upgrade` inline, **record desired version and dispatch to the worker** if idle. Return immediately — the loop keeps spinning.
- Worker performs download → verify → self-replace, then publishes `SelfUpdateRestartRequested` (success) or `SelfUpdateAttemptFailed { version }` (failure).
- Loop handles those new internal events: success → existing stop/restart path; failure → `gate.record_failure(version)`; in both cases, if `desired_version` changed while in flight, dispatch the new target (this is the F-10 fix). A second config for the *same* in-flight version is a no-op (F-18 fix).

- **Pros:** Resolves F-08 (loop never blocks), F-10 (intermediate versions tracked, not lost), and F-18 (single-flight). Keeps `!Sync` gate on the main thread (no `Arc<Mutex>` refactor). Reuses `ThreadContext`. Clear cancellation on shutdown.
- **Cons:** Most moving parts of the options; introduces 1–2 new internal events and a request/result channel; requires careful ordering decisions vs `apply_remote_config_agents` (see §3, F-07).

---

## 3. Interactions with related findings

- **F-06 (verify opens an OpAMP connection, ~8 s):** This is *why* the freeze is ~21 s and not ~8 s. Offloading (A/B/D) removes the freeze regardless, but F-06 still lengthens each attempt and means a verify-time OpAMP connection briefly coexists with the running agent's connection. Worth fixing in tandem (e.g. a lighter `verify` that skips the live OpAMP dial, or a distinct instance id) but **independent** of the offload.
- **F-07 (unnecessary sub-agent restarts):** Today `:468` runs `update()` *before* `apply_remote_config_agents` (`:487`). If the upgrade becomes async, decide explicitly: should agent-config application proceed while an AC upgrade is in flight (the process is about to restart anyway), or be gated? The cleanest behavior is: when an AC version change is dispatched, **skip redundant sub-agent churn** that the imminent restart will redo. The state machine gives a natural place to encode this.
- **F-10 (lost intermediate configs):** Direct beneficiary of Option D. With synchronous `update()`, rapid-fire configs each block-and-replace; with a single-flight controller that tracks `desired_version`, the agent converges to the latest target and doesn't run a full cycle per intermediate config.
- **F-18 (second config mid-upgrade → second download/replace):** The single-flight guard in Option D prevents a concurrent second download; a different new version supersedes the desired target and is attempted once the in-flight one settles, rather than racing.

---

## 4. Effort estimate and riskiest parts

**T-shirt: L** (~1.5–2 sprints) for the full Option D including F-10/F-18 behavior and tests. A **reduced M** (~1 sprint) is possible if scoped to "offload + single-flight, no F-10 convergence niceties," accepting that intermediate configs may still be dropped.

Riskiest parts (in rough order):

1. **Shutdown/cancellation during an in-flight download** — now that SIGTERM is handled mid-upgrade, the worker must abort cleanly (partial download cleanup, no self-replace after a stop request). New race surface.
2. **Self-replace → restart ordering** — the success event must deterministically reach the loop and trigger the existing `sub_agents.stop()` + `break SelfUpdate` path; a dropped/duplicated event must not leave a replaced-but-not-restarted binary.
3. **`BackoffGate` `!Sync`** — keeping it main-thread-owned (Option D) avoids an `Arc<Mutex>` refactor, but the boundary between "gate decision on main thread" and "execution on worker" must be drawn carefully so failure accounting stays correct under interleaving.
4. **Ordering vs `apply_remote_config_agents`** (F-07) — changing update() from sync-before-apply to async changes observable behavior; needs explicit product decision + tests.
5. **Wiring `retry()`** — if F-10 convergence is in scope, the unused `retry()`/`retry_heartbeat` should be wired (a timer arm) so a failed attempt re-drives without needing a new config push.
6. **Test surface** — the event loop is currently tested with a `MockVersionUpdater` whose `update()` is synchronous; async/worker behavior needs new test scaffolding (deterministic completion injection).

---

## 5. Prior decision to postpone — and what changed

This async/offload problem was **considered and intentionally postponed** when package-download support was added for sub-agents. The reasoning then: it touches the `select!`-driven event loop, sub-agent lifecycle, remote-config status reporting, and the interaction between in-flight downloads and incoming OpAMP messages — i.e. exactly the surface enumerated above — and sub-agent downloads did not block the *main* loop in a way that tripped Fleet Control health.

What is different now:

- **A concrete P1 with measured impact:** F-08 shows 4+ consecutive missed reporting cycles per upgrade and FC marking the agent unhealthy — a user-visible regression, not a theoretical concern.
- **The freeze is worse than expected** because of F-06: the verify subprocess's OpAMP connectivity check pushes the blocked window to ~21 s.
- **Scaffolding now exists** that makes the tractable path shorter: the `BackoffGate` single-flight/cooldown state, the (currently unwired) `retry()`/`retry_heartbeat`, and the established `ThreadContext`/`spawn_named_thread` precedent for offloaded work that reports back via event publishers.
- **The same change retires F-10** (and mitigates F-18), so the cost is amortized across multiple findings rather than spent solely on F-08.

The recommendation is therefore to revisit the postponement and schedule **Option D**, scoping F-06 as a parallel but independent fix.
