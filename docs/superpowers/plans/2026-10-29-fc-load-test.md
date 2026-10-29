# FC Load-Test Simulator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new binary that runs ~1k real on-host Agent Controls in one process against Fleet Control, each looking like a distinct host, with no-op sub-agents that report fake healthy status.

**Architecture:** Five small, opt-in hooks in the `newrelic_agent_control` library (shared HTTP client/token, health-check HTTP client only when needed, sub-agent hostname override, runner constructor with shared resources + identifier override, public list of embedded agent type ids). Everything else lives in a new binary under `agent-control/src/bin/fc_load_test/` that prepares one data dir per simulated AC and runs `AgentControlRunner::run_onhost()` on one thread per AC.

**Tech Stack:** Rust 2024, existing workspace deps only (`serde`, `serde-saphyr`, `tracing`, `tracing-subscriber`, `ctrlc`, `duration-str`, `tokio`, `tempfile` for tests).

**Spec:** `docs/superpowers/specs/2026-10-29-fc-load-test-design.md`

## Global Constraints

- Not merged; reviewability is the top priority. Every change to existing library code carries a one-line `// load-test: <why>` comment so `git grep "load-test"` lists all deviations.
- Normal binaries' behavior must not change: every hook is opt-in (`None`/absent → current code path).
- No new dependencies.
- Before calling any task done: `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo build`, and the tests of the touched module (`cargo test -p newrelic_agent_control --lib <module>` or `--bin newrelic-agent-control-fc-load-test`).
- Style per `docs/style/` and `CLAUDE.md`: `thiserror` messages lowercase, no trailing period; `tracing` messages capitalised, no trailing period, structured `snake_case` fields; spans `INFO`.
- **Do not run `git commit`.** The user commits manually; at the end of each task stop and report the diff summary.
- Target platform: macOS (also must build on Linux).
- Identity scheme: AC `i` → hostname = host_id = machine_id = `<run_prefix>-<i:04>`, `cloud_instance_id` = `""`, `fleet_id` from the template; data dir `<data_root>/<run_prefix>-<i:04>/`.

## Review Focus

1. **Rerun with the same `run_prefix` and data** must reuse instance ids. Expect identical `Identifiers` every run. Pinned in Task 6 (`identifiers_are_deterministic`).
2. **FC pushes a catalog agent type** (e.g. `newrelic/com.newrelic.infrastructure:0.1.0` with its real `config` values). It must render as a no-op, not spawn anything. Pinned in Task 6 (`shadowed_type_ignores_real_values`).
3. **Template carries `self_update`, `oci.registry` or `host_id`**. The simulator's forced values must win. Pinned in Task 6 (`forced_fields_override_template`).
4. **`fresh: true` must delete only `<data_root>/<run_prefix>-*`** and nothing else under `data_root`. Pinned in Task 6 (`fresh_wipes_only_prefix_dirs`).
5. **Ctrl-C during ramp-up** must stop ACs already started and not start more. Pinned in Task 7 (smoke test step).

---

### Task 1: Shared HTTP client and token retriever in the OpAMP HTTP builder

**Files:**
- Modify: `agent-control/src/opamp/auth/token_retriever.rs` (enum `TokenRetrieverImpl`, `impl TokenRetriever`)
- Modify: `agent-control/src/opamp/http/builder.rs` (struct `OpAMPHttpClientBuilder`, `build`)

**Interfaces:**
- Produces:
  - New variant `TokenRetrieverImpl::Shared(Arc<TokenRetrieverImpl>)`; `retrieve` delegates.
  - `pub struct SharedOpAMPHttp { pub http_client: HttpClient, pub token_retriever: Arc<TokenRetrieverImpl> }` in `opamp/http/builder.rs`, `#[derive(Clone)]`.
  - `impl<R> OpAMPHttpClientBuilder<R> { pub fn with_shared(self, shared: SharedOpAMPHttp) -> Self }`. When set, `build()` uses `shared.http_client.clone()` and `TokenRetrieverImpl::Shared(shared.token_retriever.clone())` instead of creating new ones (secret retriever not called).

- [ ] **Step 1: Write failing tests** in `builder.rs` tests:
  - `build_with_shared_does_not_call_secret_retriever`: builder with a `MockSecretRetriever` (existing mock in `opamp/secret_retriever`) expecting `retrieve` **0 times**, an `auth_config` set, and `.with_shared(SharedOpAMPHttp { http_client, token_retriever: Arc::new(TokenRetrieverImpl::Noop(TokenRetrieverNoop)) })`; `assert!(builder.build().is_ok())`.
  - In `token_retriever.rs` tests: `shared_variant_delegates`: `TokenRetrieverImpl::Shared(Arc::new(TokenRetrieverImpl::Noop(TokenRetrieverNoop))).retrieve()` equals `TokenRetrieverNoop.retrieve()`.
- [ ] **Step 2: Run** `cargo test -p newrelic_agent_control --lib opamp::` — expect compile failure (`with_shared`/`Shared` not found).
- [ ] **Step 3: Implement** the variant, struct, field `shared: Option<SharedOpAMPHttp>` (default `None` in `new`), and `with_shared`, with `// load-test` comments.
- [ ] **Step 4: Run** the same command — expect PASS. Then fmt/clippy.

### Task 2: Health checker builds its HTTP client only for `Http` checks

Without this, every sub-agent's health checker owns a `reqwest::blocking` client thread (~4k extra threads).

**Files:**
- Modify: `agent-control/src/checkers/health/on_host/health_checker.rs:33` (`OnHostHealthCheckers::try_new`)
- Modify: `agent-control/src/sub_agent/on_host/supervisor.rs:258-300` (`start_health_check`)

**Interfaces:**
- Produces: `OnHostHealthCheckers::try_new(exec_health_consumer, http_client: Option<HttpClient>, checks, start_time)`. An `Http` check with `None` → `Err(HealthCheckerError::Generic("http health check requires an http client".into()))`. The supervisor builds the client only if `checks.iter().any(|c| matches!(c, OnHostHealthCheckDefinition::Http(_)))`.

- [ ] **Step 1: Write failing tests** in `health_checker.rs` tests (update the 3 existing `try_new` call sites to pass `Some(http_client)`):
  - `try_new_file_only_without_http_client`: checks `[File(FileHealth { path: "/tmp/x".into() })]`, `None` → `Ok(Some(_))`.
  - `try_new_http_without_http_client_errors`: checks `[Http(..)]`, `None` → `Err(_)`.
- [ ] **Step 2: Run** `cargo test -p newrelic_agent_control --lib checkers::health::on_host` — expect compile failure.
- [ ] **Step 3: Implement** the signature change and the conditional build in `start_health_check`, with `// load-test` comment.
- [ ] **Step 4: Run** the same command plus `cargo test -p newrelic_agent_control --lib sub_agent::on_host` — expect PASS.

### Task 3: Sub-agent hostname override

**Files:**
- Modify: `agent-control/src/sub_agent/on_host/builder.rs:34-82` (struct field + `build`)
- Modify: the 2 struct-literal sites of `OnHostSubAgentBuilder {` (`run/on_host.rs` and its test usage) to set `hostname: None`

**Interfaces:**
- Produces: field `pub(crate) hostname: Option<String>` on `OnHostSubAgentBuilder`; `build` uses it when `Some`, else `get_hostname()`.

- [ ] **Step 1: Write failing test** in `builder.rs` tests: `build_uses_hostname_override`. Build with `hostname: Some("lt-0001".into())` and a mock OpAMP builder whose `build_and_start` expectation captures `StartSettings`. Assert `agent_description.non_identifying_attributes[HOST_NAME_ATTRIBUTE_KEY] == "lt-0001".into()`. Follow the existing test in this file that mocks `build_and_start`.
- [ ] **Step 2: Run** `cargo test -p newrelic_agent_control --lib sub_agent::on_host::builder` — expect compile failure.
- [ ] **Step 3: Implement** with `// load-test` comment.
- [ ] **Step 4: Run** — expect PASS.

### Task 4: Runner constructor with shared resources and identifier override

**Files:**
- Modify: `agent-control/src/agent_control/run.rs` (`AgentControlRunner`, `try_new`)
- Modify: `agent-control/src/agent_control/run/on_host.rs` (`run_onhost`, `opamp_client_builder`)
- Modify: `agent-control/src/command/on_host_checks/opamp.rs:27` (pass `None` to `opamp_client_builder`)

**Interfaces:**
- Consumes: `SharedOpAMPHttp` (Task 1); `OnHostSubAgentBuilder.hostname` (Task 3).
- Produces (all in `agent_control/run.rs`, `pub`):
  - `pub struct SharedRunnerResources { pub runtime: Arc<Runtime>, pub oci_client: oci::Client, pub dynamic_custom_capabilities: Vec<CustomCapability> }`, `#[derive(Clone)]`.
  - `pub struct LoadTestOverrides { pub identifiers: Identifiers, pub shared_http: Option<SharedOpAMPHttp> }`. `shared_http` is `None` when the template has no `auth_config`.
  - `pub fn AgentControlRunner::try_new_with_shared(context: RunnerContext, shared: SharedRunnerResources, overrides: LoadTestOverrides) -> Result<Self, Box<dyn Error>>`. Same as `try_new` minus runtime/OCI client creation and the reachability probe.
  - `pub fn probe_dynamic_custom_capabilities(oci_client: &oci::Client, config: &AgentControlConfig) -> Vec<CustomCapability>`, extracted from `try_new` and used by both.
  - New private field `load_test: Option<LoadTestOverrides>` on `AgentControlRunner` (`None` from `try_new`).
  - `opamp_client_builder(local_dir, opamp_config, proxy_config, repo, shared_http: Option<SharedOpAMPHttp>)`. Calls `.with_shared` when `Some`.
- In `run_onhost`: if `load_test` is `Some`, use its `identifiers` instead of `ac_identifiers(&agent_control_config)?`. Pass `shared_http` to `opamp_client_builder` and `Some(identifiers.hostname.clone())` as the sub-agent `hostname`. Otherwise current behavior.
- `try_new` refactor: build runtime + OCI client + probe, then delegate to a private `fn build(context, shared, load_test: Option<LoadTestOverrides>)` so both constructors share one body.

- [ ] **Step 1: Implement** the above with `// load-test` comments. This is wiring; existing tests are the regression net.
- [ ] **Step 2: Run** `cargo build` and `make -C agent-control test/onhost` (or the `cargo test -p newrelic_agent_control --lib agent_control::run` subset if that target needs root). Expect existing tests PASS.
- [ ] **Step 3: Run** `cargo clippy --workspace --all-targets` — no warnings.

### Task 5: Public list of embedded agent type ids

**Files:**
- Modify: `agent-control/src/agent_type/registry/local/embedded.rs`, `agent-control/src/agent_type/registry/local.rs`, `agent-control/src/agent_type/registry.rs` (re-export)

**Interfaces:**
- Produces: `pub fn embedded_agent_type_ids(env: Environment) -> Vec<AgentTypeID>` at `newrelic_agent_control::agent_type::registry::embedded_agent_type_ids`.

- [ ] **Step 1: Write failing test** `embedded_ids_include_infrastructure`: `embedded_agent_type_ids(Environment::Linux)` contains an id with `namespace() == "newrelic"`, `name() == "com.newrelic.infrastructure"`, and every id's environment is Linux, checked by comparing against `embedded_definitions(Environment::Linux)` count.
- [ ] **Step 2: Run** `cargo test -p newrelic_agent_control --lib agent_type::registry` — expect compile failure.
- [ ] **Step 3: Implement** (maps `embedded_definitions(env)` to `agent_type_id().clone()`), with `// load-test` comment.
- [ ] **Step 4: Run** — expect PASS.

### Task 6: Simulator config and per-AC data layout

**Files:**
- Create: `agent-control/src/bin/fc_load_test/main.rs` (stub `fn main() {}` + `mod config; mod layout;` for now)
- Create: `agent-control/src/bin/fc_load_test/config.rs`
- Create: `agent-control/src/bin/fc_load_test/layout.rs`
- Create: `agent-control/fc-load-test.example.yaml` (the YAML from the spec, with placeholders)
- Modify: `agent-control/Cargo.toml`: `[[bin]] name = "newrelic-agent-control-fc-load-test"`, `path = "src/bin/fc_load_test/main.rs"`

**Interfaces:**
- Consumes: `embedded_agent_type_ids` (Task 5); `Identifiers` (`opamp::instance_id::on_host::identifiers`).
- Produces:
  - `config.rs`: `#[derive(Deserialize)] struct LoadTestConfig { count: usize, ramp_up_per_second: u32, data_root: PathBuf, run_prefix: String, #[serde(default)] fresh: bool, #[serde(default = "info")] log_level: String, #[serde(default)] index_offset: usize, #[serde(default = 60s, with duration-str)] health_interval: Duration, #[serde(default)] fake_agent_types: Vec<FakeAgentTypeId>, agent_control: serde_saphyr/yaml Value }`, `struct FakeAgentTypeId { namespace, name, version: String }`, `fn LoadTestConfig::load(path: &Path) -> Result<Self, ConfigError>`. `ConfigError` is a `thiserror` struct error. Validate `count > 0`, `ramp_up_per_second > 0`, `run_prefix` non-empty and filename-safe (`[A-Za-z0-9_-]+`), and that `agent_control` deserialises as `AgentControlConfig` with `fleet_control` present.
  - `layout.rs`:
    - `struct AcLayout { pub name: String, pub local_dir: PathBuf, pub remote_dir: PathBuf, pub log_dir: PathBuf }` and `fn AcLayout::new(data_root: &Path, run_prefix: &str, index: usize) -> Self` (`name = format!("{run_prefix}-{index:04}")`, subdirs `local`, `remote`, `log`).
    - `fn identifiers(name: &str, fleet_id: &str) -> Identifiers` (all fields per Global Constraints).
    - `fn wipe_prefix(data_root: &Path, run_prefix: &str) -> io::Result<()>`. Removes only direct children whose name starts with `"{run_prefix}-"`.
    - `fn write_healthy_file(data_root: &Path) -> io::Result<PathBuf>`. Writes `<data_root>/healthy.yaml` with `healthy: true`, `status: "load-test fake healthy"`, `start_time_unix_nano` and `status_time_unix_nano` = now (field names from `checkers/health/on_host/file.rs:54-59`).
    - `fn fake_agent_type_yaml(id: &AgentTypeID or FakeAgentTypeId, healthy_file: &Path, interval: Duration) -> String`. `namespace/name/version`, `platform: host`, `operating_system: linux`, `protocol_version: "1.0"`, `variables: {}` (or the minimal form the parser accepts), `deployment: { health: { interval, checks: [{ kind: File, path }] } }`.
    - `fn prepare(layout: &AcLayout, template: &AgentControlConfig-as-Value, fleet_id, fake_types_yaml: &[(file_name, String)]) -> io::Result<()>`. Creates dirs and writes `<local_dir>/local-data/agent-control/local_config.yaml` = template with forced `host_id: <name>`, `self_update: { enabled: false }`, `oci: { registry: "127.0.0.1:1" }`, `agent_types.default_remote.repository` left as is, and every fake type into `<local_dir>/dynamic-agent-types/<namespace>-<name>-<version>.yaml`. Overwrites on each run.

- [ ] **Step 1: Write failing tests** (in `layout.rs` / `config.rs`, `tempfile::tempdir`):
  - `identifiers_are_deterministic`: `identifiers("lt-0007", "f1") == identifiers("lt-0007", "f1")`, `hostname == host_id == machine_id == "lt-0007"`, `cloud_instance_id == ""`, `fleet_id == "f1"`.
  - `forced_fields_override_template`: template containing `host_id: x`, `self_update: { enabled: true }`, `oci: { registry: real.example }`. After `prepare`, re-read `local_config.yaml` as `AgentControlConfig` and assert `host_id == name`, self-update disabled, `oci.registry` is `127.0.0.1:1`, and `fleet_control.fleet_id` is kept.
  - `fake_type_parses_with_file_health`: `AgentTypeDefinition::from_slice(fake_agent_type_yaml(...))` is `Ok`, and `health_check_enabled()` is true.
  - `shadowed_type_ignores_real_values`: build a `LocalRegistry::new(Environment::Linux, <dynamic dir from prepare>)` (or `Registry` via the public API) and fetch `newrelic/com.newrelic.infrastructure:0.1.0`. Render it with an `AgentRenderer` using a YAML config carrying that type's real `config_agent` values. Assert the rendered on-host config has no executables and no packages. If `AgentRenderer` isn't reachable from a bin, put this one test in the library under `#[cfg(test)]` next to Task 5's test.
  - `fresh_wipes_only_prefix_dirs`: create `lt-0001/`, `lt-0002/`, `other-0001/`, `healthy.yaml`; `wipe_prefix(root, "lt")`; only `other-0001/` and `healthy.yaml` remain.
  - `config_rejects_bad_prefix`: `run_prefix: "a/b"` → `Err`.
  - `example_config_parses`: `LoadTestConfig::load("fc-load-test.example.yaml")` is `Ok`.
- [ ] **Step 2: Run** `cargo test -p newrelic_agent_control --bin newrelic-agent-control-fc-load-test` — expect FAIL.
- [ ] **Step 3: Implement** `config.rs` and `layout.rs`.
- [ ] **Step 4: Run** — expect PASS. fmt/clippy.

### Task 7: Simulator main: shared resources, ramp-up, run, shutdown

**Files:**
- Modify: `agent-control/src/bin/fc_load_test/main.rs`
- Create: `agent-control/src/bin/fc_load_test/README.md` (how to run: prod/staging, against `fake-opamp-server`, splitting with `index_offset`, where data lives, how to get fresh identities)

**Interfaces:**
- Consumes: Tasks 1, 4, 6.
- Flow of `main() -> ExitCode` (errors logged with `error!`, exit 1):
  1. `LoadTestConfig::load(&args[1])`. Init `tracing_subscriber::fmt` with `EnvFilter::new(log_level)`.
  2. If `fresh`, `wipe_prefix`. `write_healthy_file`. Build fake type YAMLs for `embedded_agent_type_ids(AGENT_CONTROL_MODE_ON_HOST)` ∪ `fake_agent_types`; log `info!(count, "Shadowing agent types")` and `debug!` each id.
  3. Shared resources, once: `Arc<Runtime>` multi-thread with `worker_threads(2)`. `oci::Client::try_new(ClientConfig::default(), proxy, runtime)`. `probe_dynamic_custom_capabilities` against the **template's original** OCI/agent-type config, not the forced unreachable one. `HttpClient::new(HttpConfig::new(DEFAULT_CLIENT_TIMEOUT, DEFAULT_CLIENT_TIMEOUT, proxy))`. If `auth_config` is present, `Arc<TokenRetrieverImpl>` via `TokenRetrieverImpl::try_build(auth_config, &OnHostSecretRetriever::new(Some(fc_config), <first AC local_dir>, FileProvider), proxy)`.
  4. `ctrlc::set_handler` sets an `Arc<AtomicBool>` `stopping` and publishes `ApplicationEvent::StopRequested` to every registered `EventPublisher` (kept in `Arc<Mutex<Vec<_>>>`).
  5. For `i in index_offset..index_offset + count`, stop if `stopping`: `AcLayout::new`, `prepare`, `pub_sub()` for app events (register the publisher), build `RunnerContext { bootstrap_config: <parsed local_config>, base_paths, running_mode: AGENT_CONTROL_MODE_ON_HOST, application_event_consumer, self_replace_target: None }`. Spawn a thread named `ac-<i:04>` that enters `info_span!("simulated_ac", ac = %name)` and runs `AgentControlRunner::try_new_with_shared(ctx, shared.clone(), LoadTestOverrides { identifiers, shared_http })?.run_onhost()`, logging `error!(ac, %err, "Simulated AC failed: {err}")` on failure. Sleep `1s / ramp_up_per_second` between spawns.
  6. After ramp-up, log `info!(threads, "Ramp-up complete")` (macOS: count lines of `ps -M -p <pid>` minus header; Linux: `Threads:` in `/proc/self/status`). Then join all threads.
  7. A failed AC never stops the others.

- [ ] **Step 1: Implement** `main.rs` and `README.md`.
- [ ] **Step 2: Smoke test, local server.** `cargo run -p fake-opamp-server -- 127.0.0.1:4318` in one shell. Write `/tmp/lt-smoke.yaml` with `count: 3`, `ramp_up_per_second: 5`, `data_root: /tmp/lt-smoke`, `run_prefix: smoke`, endpoint `http://localhost:4318/<FAKE_SERVER_PATH>`, `signature_validation.enabled: false`, no `auth_config`, `poll_interval: 5s`. Run `cargo run --bin newrelic-agent-control-fc-load-test -- /tmp/lt-smoke.yaml`. Expected: `GET /admin/state` on the fake server lists 3 instances with hostnames `smoke-0000..0002`, the log shows `Ramp-up complete`, and no child processes appear (`pgrep -P <pid>` is empty).
- [ ] **Step 3: Remote config + health.** Push to one instance through the fake server's admin config endpoint an AC config with `agents: { a1: { agent_type: "newrelic/com.newrelic.infrastructure:0.1.0" } }`. Expected: the fake server shows sub-agent `a1` with its own instance and **healthy** status after ≤ `health_interval`, and no `newrelic-infra` process or OCI download in the logs.
- [ ] **Step 4: Rerun identity.** Ctrl-C, check that all threads exit and the process returns 0, then rerun the same file. Expected: same instance UIDs in the fake server state. Rerun with `fresh: true`: new UIDs.
- [ ] **Step 5: Ctrl-C during ramp-up.** Set `count: 50`, `ramp_up_per_second: 2`, Ctrl-C after ~5s. Expected: no new `simulated_ac` spans after the signal, and the process exits cleanly.
- [ ] **Step 6: Scale check on macOS.** Run `count: 1000` against the fake server and record the `Ramp-up complete` thread count and RSS (`ps -o rss`). If threads exceed ~15,000, report the numbers to the user instead of tuning silently.
- [ ] **Step 7:** `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo build`. Stop and report to the user.
