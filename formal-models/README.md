# Formal-methods learning project — newrelic-agent-control

Personal learning project: using this codebase as a substrate for learning
TLA+ / Quint by modeling real production code, finding bugs, and validating
design decisions.

> **Resuming with Claude.** Drop this file into Claude's context and tell it:
> *"I'm continuing the formal-methods learning project documented in
> `formal-models/README.md`. Current tier: <N>. Last thing I did: <X>."*
> The candidate roadmap below has all the file/line pointers Claude needs to
> navigate, plus the invariants worth proving for each candidate.

---

## Why this codebase

- Real distributed-systems code (OpAMP control-plane protocol, supervisor
  lifecycle, K8s reconciliation) — non-trivial concurrency and ordering.
- Existing `TODO` / `FIXME` markers (e.g. `agent-control/src/sub_agent.rs:292`,
  `:209`) point to known-tricky areas worth modeling.
- Linear, no-concurrency surfaces (`self-replacer`) make a gentle entry point.
- I work on this code already, so reading specs against implementation is the
  fastest path to "getting it."

## Toolchain

- **Quint** (primary) — `npm i -g @informalsystems/quint`. Modern syntax,
  good LSP, integrated test runner, Apalache backend for SMT-bounded checks.
- **TLA+ / TLC** — fall back to it when I want to learn classic TLA+ or use
  Apalache directly. VS Code TLA+ extension recommended.
- **quint-connect** — <https://github.com/quint-co/quint-connect>. Replays
  real production traces against a Quint spec. Stretch goal for Tier 3.

---

## Candidate roadmap

Ordered by recommended progression, not importance. Tier 1 is the simplest;
Tiers 2-3 reward the time investment with real distributed-systems insight.

### Tier 1 — Self-replacer (entry point)

- **Code:** `self-replacer/src/replacer.rs:92-164` (`replace_binary`)
- **Model:** `formal-models/self_replacer.qnt`
- **Status:** starter model written. Run `quint test formal-models/self_replacer.qnt`.
- **Why first:** Linear sequence, no concurrency, but real crash-recovery
  semantics. Teaches states, actions, invariants, traces, the difference
  between safety and liveness.
- **Experiments queued in the spec:**
  1. Toggle to the Windows-rename backup path, watch `safety` break.
  2. Split `atomic_rename` into rename + fsync_dir; crash between them.
  3. Model two concurrent self_replace calls (TEMP filename is a constant).

### Tier 2 — OpAMP remote-config state machine

- **Code:**
  - `agent-control/src/opamp/callbacks.rs:71-138` — `process_remote_config`
  - `agent-control/src/opamp/remote_config/hash.rs` — `ConfigState` enum
    (`Applying`, `Applied`, `Failed`)
  - `agent-control/src/agent_control.rs:377-544` — top-level apply loop
  - `agent-control/src/sub_agent.rs:386-505` — `handle_remote_config`,
    `apply_or_build_and_start_supervisor`
- **Why:** Distributed protocol with hash-based dedup, recovery after crash
  mid-apply, server/agent state divergence. Real bugs hide in this kind of
  code and it's exactly what TLA+ / Quint were designed for.
- **Goals:**
  1. Model the agent's `Applying → Applied/Failed` transitions keyed by hash.
  2. Model the server side: sends configs, receives status, retries on hash
     mismatch.
  3. Inject crashes between "apply" and "report".
- **Invariants to encode:**
  - **Idempotency on duplicate hash** — if the agent reported `Applied` for
    hash `H`, receiving `H` again must not re-enter `Applying`. (See the
    branch at `sub_agent.rs:394-400`.)
  - **No phantom Applied** — agent never reports `Applied` for a hash it
    never persisted.
  - **Convergence (liveness)** — with no further sends and no crashes,
    agent's reported hash equals server's last sent hash.
  - **Crash-recovery agreement** — after restart, persisted state and
    reported state match.

### Tier 3 — Sub-agent supervisor lifecycle

- **Code:**
  - `agent-control/src/sub_agent.rs:386-505` and the `select!` loop at
    `:309-349`
  - **Existing markers to anchor on:**
    - `TODO` at `sub_agent.rs:292` — *"We should separate the loop for OpAMP
      events and internal events into two different loops"*
    - `FIXME` at `sub_agent.rs:209` — *"only if we successfully build a
      supervisor?"*
- **Why:** Concurrent restart vs. config-arrival vs. stop-request races. The
  TODOs admit this is brittle — modeling it formally is the cleanest way to
  pin down what the spec ought to be.
- **Goals:**
  1. Model the multiplexed event loop with three input channels: remote
     config, stop, health.
  2. Model `apply(self, ...) -> Self` (consuming-and-returning the
     supervisor) as an atomic transition.
  3. Inject concurrent stop arrivals during config apply.
- **Invariants:**
  - Supervisor uniqueness: at most one active supervisor per agent ID.
  - No orphan processes after stop.
  - Apply atomicity from the caller's view.
- **Stretch:** instrument the real `OpAMPEvent` channel, emit traces, replay
  through quint-connect to prove the spec matches production behavior.

### Lower-priority candidates (for later)

| Area | Code |
|---|---|
| Effective config reporting & hash dedup | `agent-control/src/opamp/effective_config/loader.rs`, `agent-control/src/opamp/callbacks.rs:217-229` |
| K8s reflector watch consistency | `agent-control/src/k8s/reflectors.rs:70-196` |
| Config layering precedence | `agent-control/src/sub_agent/remote_config_parser.rs:75-100` |
| Health-checker aggregation | `agent-control/src/checkers/health/health_checker.rs:62-80` |

---

## Workflow with Claude

1. Open Claude in this repo.
2. Have it read `formal-models/README.md` and the current model file(s).
3. Tell it the current tier, last action, and what you want next:
   - *"Tier 1 is working. Now I want to add fsync_dir as a separate action
     and see if safety still holds with crashes between the two."*
   - *"Move me to Tier 2. Sketch the agent half of the OpAMP state machine
     in Quint, grounded in `ConfigState` and `handle_remote_config`."*
4. Iterate. Break the model intentionally. Read the counterexample trace.
   Update the spec. Repeat.

## Resources

- Quint book — <https://quint-lang.org/docs/lang>
- Quint-connect — <https://github.com/quint-co/quint-connect>
- Lamport's TLA+ video course — <https://lamport.azurewebsites.net/video/videos.html>
- *Specifying Systems* (Lamport, free PDF)
- *Why Amazon Chose TLA+* — Newcombe et al., 2015 (industrial case studies)
- This repo:
  - `docs/opamp-communication-flows/ac-remote-update.md` — OpAMP flow doc
  - `memory-leak-investigation.md` — context on long-running behavior

## Decisions logged

- **Quint over TLA+ as primary tool.** Modern syntax, better LSP/test loop,
  and quint-connect's runtime-conformance angle is unique. Will dip into
  TLA+ if I want PlusCal or to compare proof styles.
- **Started with `self-replacer`.** Linear protocol, small surface, real
  crash-recovery semantics, no concurrency to confuse the modeling intuition.
- **Pedagogy: break the model on purpose.** The lessons live in the
  counterexample traces, not in the proofs that succeed.
