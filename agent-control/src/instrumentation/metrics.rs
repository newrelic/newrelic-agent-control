//! Operational metrics plumbing for Agent Control self-instrumentation.
//!
//! This module currently only holds the machinery to register the active OTel meter provider
//! and force-flush it before process replacement (self-update via `exec()` bypasses `Drop`).
//! Business-level `record_*` instruments are added on top of this in a follow-up.

use opentelemetry_sdk::metrics::SdkMeterProvider;
use std::sync::OnceLock;

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
