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
use opentelemetry::metrics::{Counter, MeterProvider as _};
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

/// Force-flush all pending metric data synchronously.
/// No-op when self-instrumentation is not configured.
pub fn flush() {
    if let Some(provider) = METER_PROVIDER.get() {
        if let Err(e) = provider.force_flush() {
            tracing::warn!(error = %e, "failed to flush OTLP metrics");
        }
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
            INST.get_or_init(|| {
                meter()
                    .u64_counter($metric)
                    .with_description($desc)
                    .build()
            })
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

/// Record a remote config message received from Fleet Control via OpAMP.
pub fn record_remote_config_received() {
    remote_config_received().add(1, &[]);
}

/// Record a remote config successfully applied to a sub-agent.
pub fn record_remote_config_applied(agent_id: &str) {
    remote_config_applied().add(1, &[KeyValue::new("agent_id", agent_id.to_string())]);
}

/// Record a remote config rejected due to invalid signature or validation failure.
pub fn record_remote_config_rejected(reason: &str) {
    remote_config_rejected().add(1, &[KeyValue::new("reason", reason.to_string())]);
}

/// Record a successful OpAMP connection (initial or reconnect).
pub fn record_opamp_connected() {
    opamp_reconnects().add(1, &[]);
}

/// Record an OpAMP connection failure / disconnect.
pub fn record_opamp_disconnected() {
    opamp_disconnects().add(1, &[]);
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
