//! Operational metrics for Agent Control self-instrumentation.
//!
//! All helpers are no-ops when self-instrumentation is not configured —
//! the global OTel meter provider falls back to a no-op implementation.
//!
//! Instruments are lazily initialized on first use and cached for the
//! lifetime of the process, avoiding per-call SDK lookup overhead and
//! instrument description-conflict warnings.
//!
//! These hooks also serve as the blueprint for the Phase 2 custom Events
//! taxonomy (NR-581620).

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use std::sync::OnceLock;

const METER_NAME: &str = "agent-control";

// ── Provider reference for flush ──────────────────────────────────────────

/// Holds a reference to the active SdkMeterProvider so we can call
/// force_flush() before process replacement (self-update via exec bypasses Drop).
static METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

/// Register the provider for flush access. Called from otel.rs after
/// set_meter_provider. Silently ignored if called more than once (e.g. tests).
pub fn register_provider(provider: SdkMeterProvider) {
    let _ = METER_PROVIDER.set(provider);
}

/// Force-flush all pending metric data, bounded by `flush::FLUSH_TIMEOUT` so a hung OTLP
/// endpoint can't stall the caller (this runs on the self-update path, right before `exec()`).
/// No-op when self-instrumentation is not configured.
pub fn flush() {
    if let Some(provider) = METER_PROVIDER.get() {
        let provider = provider.clone();
        crate::instrumentation::flush::force_flush_with_timeout(
            "metrics",
            crate::instrumentation::flush::FLUSH_TIMEOUT,
            move || provider.force_flush(),
        );
    }
}

fn meter() -> opentelemetry::metrics::Meter {
    opentelemetry::global::meter(METER_NAME)
}

// ── Cached instrument accessors ────────────────────────────────────────────
// Each instrument is created once and cached; subsequent calls reuse the handle.

macro_rules! counter {
    ($name:ident, $metric:expr, $desc:expr) => {
        fn $name() -> &'static Counter<u64> {
            static INST: OnceLock<Counter<u64>> = OnceLock::new();
            INST.get_or_init(|| meter().u64_counter($metric).with_description($desc).build())
        }
    };
}

// Unlike `counter!`, a gauge reports the current value of something that can go up or down
// (e.g. "how many sub-agents are running right now"), not a running total of events. Naming
// deliberately omits the `_total` suffix `counter!` metrics use, since that suffix implies
// monotonic accumulation.
macro_rules! gauge {
    ($name:ident, $metric:expr, $desc:expr) => {
        fn $name() -> &'static Gauge<u64> {
            static INST: OnceLock<Gauge<u64>> = OnceLock::new();
            INST.get_or_init(|| meter().u64_gauge($metric).with_description($desc).build())
        }
    };
}

// Same as `gauge!` but for values where truncating to a u64 would lose meaningful precision
// (e.g. CPU usage percentage).
macro_rules! gauge_f64 {
    ($name:ident, $metric:expr, $desc:expr) => {
        fn $name() -> &'static Gauge<f64> {
            static INST: OnceLock<Gauge<f64>> = OnceLock::new();
            INST.get_or_init(|| meter().f64_gauge($metric).with_description($desc).build())
        }
    };
}

counter!(
    agents_started,
    "agent_control.agents.started_total",
    "Number of sub-agents started by Agent Control"
);
counter!(
    agents_stopped,
    "agent_control.agents.stopped_total",
    "Number of sub-agents stopped"
);
counter!(
    agents_restarts,
    "agent_control.agents.restarts_total",
    "Number of sub-agent restart attempts by the supervisor"
);
gauge!(
    agents_managed,
    "agent_control.agents.managed",
    "Current number of sub-agents managed by this Agent Control instance"
);
counter!(
    remote_config_received,
    "agent_control.remote_config.received_total",
    "Remote configuration messages received from Fleet Control via OpAMP"
);
counter!(
    remote_config_applied,
    "agent_control.remote_config.applied_total",
    "Remote configurations successfully applied to sub-agents"
);
counter!(
    remote_config_rejected,
    "agent_control.remote_config.rejected_total",
    "Remote configurations rejected (invalid signature or validation failure)"
);
counter!(
    remote_config_failed,
    "agent_control.remote_config.failed_total",
    "Remote configurations that passed validation but failed while being applied"
);
counter!(
    opamp_reconnects,
    "agent_control.opamp.reconnects_total",
    "Number of times the OpAMP connection was (re)established"
);
counter!(
    opamp_disconnects,
    "agent_control.opamp.disconnects_total",
    "Number of times the OpAMP connection was lost or failed"
);
counter!(
    updates_attempted,
    "agent_control.updates.attempted_total",
    "Agent update operations attempted"
);
counter!(
    updates_succeeded,
    "agent_control.updates.succeeded_total",
    "Agent update operations completed successfully"
);
counter!(
    updates_failed,
    "agent_control.updates.failed_total",
    "Agent update operations that failed"
);
counter!(
    agent_health_transitions,
    "agent_control.agents.health_transitions_total",
    "Number of sub-agent health status transitions (healthy<->unhealthy)"
);
counter!(
    agent_health_checks,
    "agent_control.agents.health_checks_total",
    "Health checks performed, fired on every check regardless of outcome or whether status \
     changed - unlike health_transitions_total, this is a true liveness/heartbeat signal since \
     it only stops incrementing if the health checker itself stops running. Covers both Agent \
     Control's own health checker and every sub-agent's, since both share the same code path."
);
counter!(
    gc_resources_deleted,
    "agent_control.gc.resources_deleted_total",
    "Kubernetes resources deleted by the garbage collector"
);
counter!(
    gc_resources_skipped,
    "agent_control.gc.resources_skipped_total",
    "Kubernetes resources skipped during garbage collection (not deleted)"
);
counter!(
    http_server_errors,
    "agent_control.http_server.errors_total",
    "Errors encountered running or stopping the Agent Control HTTP status server"
);
counter!(
    http_server_requests,
    "agent_control.http_server.requests_total",
    "Requests served by the Agent Control HTTP status server, so error rate can be computed against total volume"
);
gauge_f64!(
    process_cpu_usage,
    "agent_control.process.cpu_usage_percent",
    "CPU usage of the process, as a percentage of one core (may exceed 100 on multi-threaded \
     work spread across cores). Covers Agent Control itself and its on-host sub-agents' child \
     processes only - k8s sub-agents run as separate pods, not something AC can read locally."
);
gauge!(
    process_memory,
    "agent_control.process.memory_bytes",
    "Resident set size (RSS) of the process. Same on-host-only scope as cpu_usage_percent."
);

// ── Public API ─────────────────────────────────────────────────────────────

/// Record a sub-agent being started by the supervisor.
pub fn record_agent_started(agent_type: &str) {
    agents_started().add(1, &[KeyValue::new("agent_type", agent_type.to_string())]);
}

/// Record a sub-agent stopping. `reason` should be one of:
/// `"graceful"`, `"crash"`, `"update"`, `"removed"`, `"restart_policy_exceeded"`.
pub fn record_agent_stopped(agent_type: &str, reason: &str) {
    agents_stopped().add(
        1,
        &[
            KeyValue::new("agent_type", agent_type.to_string()),
            KeyValue::new("reason", reason.to_string()),
        ],
    );
}

/// Record a supervisor restart attempt (restart policy triggered).
pub fn record_agent_restarted(agent_type: &str) {
    agents_restarts().add(1, &[KeyValue::new("agent_type", agent_type.to_string())]);
}

/// Record the current number of sub-agents managed by this instance. Unlike the started/stopped/
/// restarts counters above (cumulative totals of past events), this is a point-in-time snapshot -
/// call it whenever the managed set changes (bootstrap, and after every remote-config apply) so
/// `latest()` in NRQL always reflects what's actually running right now, not an event history.
pub fn record_agents_managed(count: u64) {
    agents_managed().record(count, &[]);
}

/// Record a remote config message received from Fleet Control via OpAMP.
pub fn record_remote_config_received() {
    remote_config_received().add(1, &[]);
}

/// Record a remote config successfully applied. `agent_id` identifies whichever
/// agent applied it — Agent Control itself (its own fleet-level config, see
/// `defaults::AGENT_CONTROL_ID`) or an individual sub-agent applying config
/// targeted specifically at it.
pub fn record_remote_config_applied(agent_id: &str) {
    remote_config_applied().add(1, &[KeyValue::new("agent_id", agent_id.to_string())]);
}

/// Record a remote config rejected due to invalid signature or validation failure,
/// i.e. before any apply attempt was made.
pub fn record_remote_config_rejected(reason: &str) {
    remote_config_rejected().add(1, &[KeyValue::new("reason", reason.to_string())]);
}

/// Record a remote config that passed validation but failed while being applied
/// (e.g. sub-agent build errors, version update failure). `agent_id` follows the
/// same convention as [`record_remote_config_applied`].
pub fn record_remote_config_failed(agent_id: &str, reason: &str) {
    remote_config_failed().add(
        1,
        &[
            KeyValue::new("agent_id", agent_id.to_string()),
            KeyValue::new("reason", reason.to_string()),
        ],
    );
}

/// Record a successful OpAMP connection (initial or reconnect).
pub fn record_opamp_connected() {
    opamp_reconnects().add(1, &[]);
}

/// Record an OpAMP connection failure / disconnect. `reason` should be a stable,
/// low-cardinality string identifying the failure category (e.g.
/// `"invalid_license_key"`, `"forbidden"`, `"transport_error"`).
pub fn record_opamp_disconnected(reason: &str) {
    opamp_disconnects().add(1, &[KeyValue::new("reason", reason.to_string())]);
}

/// Record an update operation being attempted.
pub fn record_update_attempted(agent_type: &str, to_version: &str) {
    updates_attempted().add(
        1,
        &[
            KeyValue::new("agent_type", agent_type.to_string()),
            KeyValue::new("to_version", to_version.to_string()),
        ],
    );
}

/// Record a successful update.
pub fn record_update_succeeded(agent_type: &str, from_version: &str, to_version: &str) {
    updates_succeeded().add(
        1,
        &[
            KeyValue::new("agent_type", agent_type.to_string()),
            KeyValue::new("from_version", from_version.to_string()),
            KeyValue::new("to_version", to_version.to_string()),
        ],
    );
}

/// Record a failed update. `error_code` should be a stable, low-cardinality
/// string identifying the failure category (e.g. `"install_failed"`,
/// `"verify_failed"`, `"replace_failed"`, `"helm_patch_failed"`).
pub fn record_update_failed(agent_type: &str, error_code: &str) {
    updates_failed().add(
        1,
        &[
            KeyValue::new("agent_type", agent_type.to_string()),
            KeyValue::new("error_code", error_code.to_string()),
        ],
    );
}

/// Record a sub-agent health status transition (healthy<->unhealthy). Callers
/// are expected to only invoke this on an actual state change, not every poll.
pub fn record_agent_health_transition(agent_type: &str, healthy: bool) {
    agent_health_transitions().add(
        1,
        &[
            KeyValue::new("agent_type", agent_type.to_string()),
            KeyValue::new("status", if healthy { "healthy" } else { "unhealthy" }),
        ],
    );
}

/// Record a health check tick for `agent_id`, regardless of outcome. Call this unconditionally
/// on every health check, not just on transitions - see `agent_health_checks` for why.
pub fn record_agent_health_check(agent_id: &str, agent_type: &str) {
    agent_health_checks().add(
        1,
        &[
            KeyValue::new("agent_id", agent_id.to_string()),
            KeyValue::new("agent_type", agent_type.to_string()),
        ],
    );
}

/// Record a Kubernetes resource deleted by the garbage collector.
pub fn record_gc_resource_deleted(resource_type: &str) {
    gc_resources_deleted().add(
        1,
        &[KeyValue::new("resource_type", resource_type.to_string())],
    );
}

/// Record a Kubernetes resource skipped during garbage collection. `reason`
/// should be a stable, low-cardinality string (e.g. `"no_name"`,
/// `"missing_owned_by_annotation"`, `"invalid_agent_id"`,
/// `"missing_type_annotation"`, `"missing_api_resource"`).
pub fn record_gc_resource_skipped(resource_type: &str, reason: &str) {
    gc_resources_skipped().add(
        1,
        &[
            KeyValue::new("resource_type", resource_type.to_string()),
            KeyValue::new("reason", reason.to_string()),
        ],
    );
}

/// Record an error running or stopping the HTTP status server. `stage` should
/// be one of `"run"`, `"start"`, `"stop"`; `error_kind` a stable,
/// low-cardinality classification of the failure.
pub fn record_http_server_error(stage: &str, error_kind: &str) {
    http_server_errors().add(
        1,
        &[
            KeyValue::new("stage", stage.to_string()),
            KeyValue::new("error_kind", error_kind.to_string()),
        ],
    );
}

/// Record a request served by the HTTP status server, regardless of outcome.
/// `endpoint` identifies which route was hit (e.g. `"status"`).
pub fn record_http_server_request(endpoint: &str) {
    http_server_requests().add(1, &[KeyValue::new("endpoint", endpoint.to_string())]);
}

/// Record a CPU/memory sample for a process. `agent_id`/`agent_type` follow the same convention
/// as [`record_agent_health_check`] - Agent Control itself uses `defaults::AGENT_CONTROL_ID`.
pub fn record_process_resources(
    agent_id: &str,
    agent_type: &str,
    cpu_percent: f64,
    memory_bytes: u64,
) {
    let attributes = [
        KeyValue::new("agent_id", agent_id.to_string()),
        KeyValue::new("agent_type", agent_type.to_string()),
    ];
    process_cpu_usage().record(cpu_percent, &attributes);
    process_memory().record(memory_bytes, &attributes);
}
