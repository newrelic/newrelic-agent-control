//! Operational metrics for Agent Control self-instrumentation.
//!
//! All helpers are no-ops when self-instrumentation is not configured —
//! the global OTel meter provider falls back to a no-op implementation.
//!
//! Call sites do not need to know whether OTLP is enabled; they just call
//! the relevant helper and the metric flows (or silently drops) accordingly.
//!
//! These hooks also serve as the blueprint for the Phase 2 custom Events
//! taxonomy (NR-581620).

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;

const METER_NAME: &str = "agent-control";

fn meter() -> opentelemetry::metrics::Meter {
    opentelemetry::global::meter(METER_NAME)
}

// ── Agent lifecycle ────────────────────────────────────────────────────────

/// Record a sub-agent being started by the supervisor.
pub fn record_agent_started(agent_type: &str) {
    meter()
        .u64_counter("agent_control.agents.started_total")
        .with_description("Number of sub-agents started by Agent Control")
        .build()
        .add(1, &[KeyValue::new("agent_type", agent_type.to_string())]);
}

/// Record a sub-agent stopping. `reason` should be one of:
/// `"graceful"`, `"crash"`, `"update"`, `"removed"`.
pub fn record_agent_stopped(agent_type: &str, reason: &str) {
    meter()
        .u64_counter("agent_control.agents.stopped_total")
        .with_description("Number of sub-agents stopped")
        .build()
        .add(
            1,
            &[
                KeyValue::new("agent_type", agent_type.to_string()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
}

/// Record a supervisor restart attempt (restart policy triggered).
pub fn record_agent_restarted(agent_type: &str) {
    meter()
        .u64_counter("agent_control.agents.restarts_total")
        .with_description("Number of sub-agent restart attempts by the supervisor")
        .build()
        .add(1, &[KeyValue::new("agent_type", agent_type.to_string())]);
}

// ── Remote config / Fleet Control ─────────────────────────────────────────

/// Record a remote config message received from Fleet Control via OpAMP.
pub fn record_remote_config_received() {
    meter()
        .u64_counter("agent_control.remote_config.received_total")
        .with_description("Remote configuration messages received from Fleet Control via OpAMP")
        .build()
        .add(1, &[]);
}

/// Record a remote config successfully applied to a sub-agent.
pub fn record_remote_config_applied(agent_id: &str) {
    meter()
        .u64_counter("agent_control.remote_config.applied_total")
        .with_description("Remote configurations successfully applied to sub-agents")
        .build()
        .add(1, &[KeyValue::new("agent_id", agent_id.to_string())]);
}

/// Record a remote config rejected due to invalid signature or validation failure.
pub fn record_remote_config_rejected(reason: &str) {
    meter()
        .u64_counter("agent_control.remote_config.rejected_total")
        .with_description("Remote configurations rejected (invalid signature or validation failure)")
        .build()
        .add(1, &[KeyValue::new("reason", reason.to_string())]);
}

// ── OpAMP connectivity ─────────────────────────────────────────────────────

/// Record a successful OpAMP connection (initial or reconnect).
pub fn record_opamp_connected() {
    meter()
        .u64_counter("agent_control.opamp.reconnects_total")
        .with_description("Number of times the OpAMP connection was (re)established")
        .build()
        .add(1, &[]);
}

/// Record an OpAMP connection failure / disconnect.
pub fn record_opamp_disconnected() {
    meter()
        .u64_counter("agent_control.opamp.disconnects_total")
        .with_description("Number of times the OpAMP connection was lost or failed")
        .build()
        .add(1, &[]);
}

// ── Agent updates ──────────────────────────────────────────────────────────

/// Record an update operation being attempted.
pub fn record_update_attempted(agent_type: &str, to_version: &str) {
    meter()
        .u64_counter("agent_control.updates.attempted_total")
        .with_description("Agent update operations attempted")
        .build()
        .add(
            1,
            &[
                KeyValue::new("agent_type", agent_type.to_string()),
                KeyValue::new("to_version", to_version.to_string()),
            ],
        );
}

/// Record a successful update.
pub fn record_update_succeeded(agent_type: &str, from_version: &str, to_version: &str) {
    meter()
        .u64_counter("agent_control.updates.succeeded_total")
        .with_description("Agent update operations completed successfully")
        .build()
        .add(
            1,
            &[
                KeyValue::new("agent_type", agent_type.to_string()),
                KeyValue::new("from_version", from_version.to_string()),
                KeyValue::new("to_version", to_version.to_string()),
            ],
        );
}

/// Record a failed update.
pub fn record_update_failed(agent_type: &str, error_code: &str) {
    meter()
        .u64_counter("agent_control.updates.failed_total")
        .with_description("Agent update operations that failed")
        .build()
        .add(
            1,
            &[
                KeyValue::new("agent_type", agent_type.to_string()),
                KeyValue::new("error_code", error_code.to_string()),
            ],
        );
}
