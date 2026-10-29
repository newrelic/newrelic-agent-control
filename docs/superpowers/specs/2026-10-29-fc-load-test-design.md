# Fleet Control load-test simulator — design

## Intent

Load-test the **Fleet Control (FC) server**, not Agent Control. One process on one machine
simulates ~1,000 Agent Controls (ACs) managing ~4,000 sub-agents, each looking like a separate
host to FC. OpAMP traffic, instance registration and remote config delivery must be real. The
run is started and left running while the FC operators observe their side; the tool itself does
not measure or assert anything.

The code is a throwaway: it will **not be merged**. The overriding quality bar is that it is
**easy to review and understand** — a reviewer should be able to see every way the simulator
deviates from a real on-host AC.

## Goals

- Simulate N ACs (target 1k) in a single process, each running the real `AgentControl` loop with
  the real on-host wiring (`AgentControlRunner::run_onhost`).
- Each AC looks like a distinct machine: own hostname, host id, instance id(s).
- All ACs share the same system identity (client id + private key + token URL). Confirmed:
  FC accepts one identity for all simulated instances.
- Sub-agents come from **remote config pushed by FC** and are created by the existing code
  (target ~4 per AC, decided by what FC pushes).
- Sub-agents never spawn processes and never download OCI packages.
- Sub-agents report **fake, always-healthy health** to FC, so health traffic is exercised.
- Primary target machine: **the user's Mac** (macOS).
- Target environment (prod or staging) and all test knobs are set in one YAML file.
- Runs until Ctrl-C; Ctrl-C shuts all ACs down gracefully.
- Identities are stable across runs unless the data is deleted or the run prefix is changed.

## Non-goals

- Measuring latency/throughput or producing a report (FC operators observe server-side).
- Load-testing the system identity / token service.
- Testing Kubernetes mode, self-update, OCI packages, or real sub-agent supervision.
- Production-quality code, tests beyond what's needed to trust the run, or merging to `main`.

## Anti-goals (failure even if it "works")

- The simulator loads the local machine noticeably (process spawning, OCI downloads, per-AC
  tokio runtimes, cloud-metadata probes).
- All ACs look like the same host to FC (same hostname / host id).
- Hidden behavior changes in the library that a reviewer can't easily spot.
- 5k independent token fetches hammering the identity service.

## What the code already gives us (verified)

- `AgentControl::run()` is blocking; one OS thread per simulated AC is the natural unit.
- On-host agent type with a `deployment` block containing **no executables, packages or health**
  parses and the supervisor starts nothing. `deployment` itself is mandatory in the parser, so
  "no deployment section" is realised as an empty on-host deployment, e.g.:

  ```yaml
  namespace: newrelic
  name: com.newrelic.loadtest
  version: 0.1.0
  platform: host
  operating_system: linux   # matches the build target
  protocol_version: "1.0"
  variables:
    common: {}
  deployment: {}
  ```
- Dynamic agent types are loaded from `<local_dir>/dynamic-agent-types/` and take precedence over
  embedded ones.
- Instance ids are persisted in each AC's data dirs and reused as long as the identifiers
  (host id, hostname, fleet id) don't change. Remote config is persisted too, so a restarted
  simulated AC resumes its last config like a rebooted host.
- Each OpAMP client (AC + every sub-agent) currently builds its own `TokenRetrieverImpl`
  (`agent-control/src/opamp/http/builder.rs:92`). nr-auth 0.7.0's `TokenRetrieverWithCache`
  holds the token in a `Mutex<Option<Token>>` and refreshes under the lock, so a single instance
  is safe to share across threads and refreshes once on expiry.

## Shape

### New binary

`agent-control/src/bin/main_fc_load_test.rs` (plus, if it grows, a small sibling module).
Registered as a `[[bin]]` in `agent-control/Cargo.toml`. Uses only the library's public API plus
the hooks listed below.

Startup:
1. Parse the load-test YAML (path as the only CLI arg).
2. Init one global `tracing` subscriber.
3. Build the shared resources once: one tokio runtime, one OCI client, one token retriever.
4. For each AC index `i` in `0..count`, ramping at the configured rate:
   - Create/reuse `<data_root>/<prefix>-<i>/{local,remote,log}`.
   - Write the AC local config (from the template in the YAML, see below) with
     `host_id = <prefix>-<i>` and the fleet id.
   - Write the fake agent type(s) into that AC's dynamic agent type dir.
   - Spawn a named thread that builds an `AgentControlRunner` with the shared resources and
     identity overrides, and calls `run_onhost()`.
5. One Ctrl-C handler fans out a stop `ApplicationEvent` to every AC; main joins all threads.

### Library hooks (the only changes to existing code)

Each one is small, opt-in, and leaves the normal binaries' behavior unchanged. They should
be easy to find in the diff (e.g. grouped and marked with a short `// load-test` comment).

1. **Shared runtime + shared OCI probe result.** A constructor on `AgentControlRunner` that
   takes an existing `Arc<Runtime>`, `oci::Client` and the precomputed dynamic custom
   capabilities. The simulator runs the OCI reachability probe **once** at startup and passes
   the result to every AC, so FC still sees `RemoteAgentTypeRepoReachable` when applicable
   (normal `try_new` is unchanged). Without it every AC creates its own multi-threaded tokio
   runtime (≈ CPU-count threads each) and probes the registry.
2. **Identifier override.** An optional full `Identifiers` on the runner (all five fields:
   `hostname`, `host_id`, `machine_id` = host id, `cloud_instance_id` = empty, `fleet_id`),
   always filled the same way so instance-id reuse across runs works
   (`opamp/instance_id/getter.rs:67` compares the whole struct). When present, `run_onhost`
   uses it instead of `ac_identifiers()`, skipping system and cloud-metadata detection.
3. **Sub-agent hostname.** `OnHostSubAgentBuilder::build` calls `get_hostname()` directly
   (`sub_agent/on_host/builder.rs:77`); give the builder an optional hostname, set from the
   override, so sub-agents report the simulated host too.
4. **Shared HTTP plumbing.** Allow `OpAMPHttpClientBuilder` to be given a pre-built, shared
   token retriever (e.g. a `TokenRetrieverImpl` variant wrapping `Arc<TokenRetrieverHttp>`)
   and a shared `HttpClient`, plumbed from the runner to `opamp_client_builder()` and used by
   the AC client and all its sub-agent clients. `HttpClient` wraps a `reqwest::blocking`
   client, which owns a background thread; sharing one (it is `Clone`) instead of building
   two per OpAMP client is what keeps the thread count manageable.

Everything else (config loading, remote config handling, signature validation, sub-agent
creation, effective config, health reporting) runs unchanged.

### Fake agent types: shadow everything

The simulator generates a no-op definition (empty `deployment`, same id/version) for **every
embedded agent type** plus any extra ids listed in `fake_agent_types`, and writes them into each
AC's dynamic agent type dir. Dynamic types take precedence, so whatever catalog type FC pushes
becomes a no-op. Unknown variables in a shadowed type are ignored
(`agent_type/variable/tree.rs:222`). The template also points `agent_types.default_remote` /
`oci.registry` at an unreachable address so an unexpected type fails fast instead of being
downloaded. At startup the simulator logs the list of shadowed ids.

### Fake health

Every fake agent type carries an on-host `health` block with a single `File` check pointing at
one file the simulator writes once at startup (`<data_root>/healthy.yaml`, in the format the
file health checker expects as healthy). No process, no HTTP. Interval is a top-level knob
(`health_interval`, default 60s) templated into the generated types. The health checker
starts for a supervisor with zero executables (verified, `sub_agent/on_host/supervisor.rs:372`).
It does, however, always build its own `HttpClient` (one `reqwest::blocking` thread per
sub-agent), so a fifth small hook builds that client only when an `Http` check is configured.

### Load-test YAML

One file. The `agent_control` block is a **real AC on-host config template**, so any AC knob
read by `run_onhost` (poll interval, signature validation, uptime report, …) can be tweaked
without new code. The simulator only fills in per-AC fields and forces `self_update.enabled:
false` (the default is `true`, which would download AC packages and replace the simulator
binary). `log` and `self_instrumentation` are only read when tracing is initialised, so logging
is configured by the top-level `log_level` instead.

```yaml
count: 1000                 # number of simulated ACs
ramp_up_per_second: 20      # ACs started per second
data_root: /tmp/fc-load-test
run_prefix: lt-2026-10-29   # change to get brand-new identities without deleting data
fresh: false                # true: wipe data_root/<run_prefix>-* before starting
log_level: info             # global tracing filter for the whole process
index_offset: 0             # first AC index; lets two processes split the load (0 and 500)
health_interval: 60s        # fake File health check interval

fake_agent_types:           # extra ids to shadow on top of all embedded types
  - namespace: newrelic
    name: com.newrelic.loadtest
    version: 0.1.0

agent_control:              # standard AC config; host_id is overwritten per AC
  fleet_control:
    endpoint: https://opamp.service.newrelic.com/v1/opamp    # or staging
    poll_interval: 30s
    fleet_id: <FLEET_ID>
    signature_validation:
      enabled: true
    auth_config:
      token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token
      client_id: <CLIENT_ID>
      provider: local
      private_key_path: /path/to/key.pem
  agents: {}                # sub-agents come from FC remote config
```

All ACs join the single `fleet_id`, so one deployment from FC reaches all of them.

### Identity and reruns

- AC `i` uses hostname and host id `<run_prefix>-<i:04>` and data dir
  `<data_root>/<run_prefix>-<i:04>/`.
- Rerun with the same prefix and data: the same ACs and sub-agents reconnect.
- Change `run_prefix` or set `fresh: true`: new identities.

## Constraints

- One process, mostly idle OS threads. Unshared, an AC with 4 sub-agents costs ~20 threads
  (AC loop, 5 OpAMP pollers, sub-agent loops, plus one thread per `reqwest::blocking` client:
  OpAMP, token retriever, signature validator). That is ~20k at target scale, above macOS's
  16,384-per-process cap (`kern.num_taskthreads`). Hook 4 brings it down to roughly 1 + 5 + 4
  ≈ 10 per AC, plus 4 health-checker threads for fake health: ~14 per AC, ~14k total. That
  fits on macOS but with little headroom. Mitigations, no extra code: lower `count`, or run
  two processes with `index_offset: 0` / `index_offset: 500` and `count: 500` each. The
  simulator logs the process thread count after ramp-up so the real number is known.
- Disk use: small YAML/instance-id files per AC under `data_root`.
- Must build with `cargo build` and pass `cargo clippy --workspace --all-targets` and
  `cargo fmt --check`, per repo conventions, even though it won't be merged.
- Follow the repo's error/log style (`docs/style/`).

## Left to the builder

- Exact names and signatures of the library hooks, as long as they stay small and opt-in.
- Whether the shared-token change is a new enum variant or an `Arc` inside the builder.
- How the per-AC config template is merged/written (serde round-trip is fine).
- Thread naming, logging layout (an `INFO` span carrying the AC index is suggested so log
  lines can be attributed), and ramp-up mechanics.
- Whether the fake agent type YAML is inlined in the binary or generated from
  `fake_agent_types`.
