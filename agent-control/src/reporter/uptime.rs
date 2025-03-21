//! This module contains the [`UptimeReporter`] type.
//!
//! The [`UptimeReporter`] is responsible for periodically reporting the uptime of an agent. It is
//! intended to be used only when self-instrumentation is enabled. The report is done using an
//! internal [`StartedThreadContext`].
//!
//! Place it where an agent gets its runtime defined and before it blocks, like in
//! the initialization routines of Agent Control or inside [`SubAgent::runtime`].
//!
//! Stop it with [`UptimeReporter::stop`] after the agent runtime unblocks to avoid any possible
//! resource leaks if the reporter is just dropped, since the [`StartedThreadContext`] could continue running.
use std::time::{Duration, Instant};

use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};
use tracing::{error, info, trace};

use crate::{
    agent_control::agent_id::AgentID,
    event::{cancellation::CancellationMessage, channel::EventConsumer},
    instrumentation::tracing::TracingGuard,
    utils::thread_context::{NotStartedThreadContext, StartedThreadContext},
};

const THREAD_NAME: &str = "uptime-reporter";

/// The uptime reporter structure.
///
/// Use it through the exposed methods [`UptimeReporter::new`] and [`UptimeReporter::stop`].
pub struct UptimeReporter(Option<StartedThreadContext>);

impl TracingGuard for UptimeReporter {}

impl Drop for UptimeReporter {
    fn drop(&mut self) {
        let Some(started_thread_context) = self.0.take() else {
            return;
        };

        let _ = started_thread_context
            .stop()
            .inspect(|_| info!("uptime reporter thread stopped"))
            .inspect_err(|error_msg| {
                error!(
                    err = %error_msg,
                    "stopping {} thread", THREAD_NAME
                )
            });
    }
}

impl UptimeReporter {
    /// Create a new [UptimeReporter], which will report the uptime of the agent at the given `interval`.
    pub fn start(agent_id: &AgentID, interval: UptimeReporterInterval) -> Self {
        Self(
            NotStartedThreadContext::new(
                format!("{agent_id}-uptime-reporter"),
                Self::report_uptime(Instant::now(), interval.into()),
            )
            .start()
            .into(),
        )
    }

    fn report_uptime(
        start_time: Instant,
        interval: Duration,
    ) -> impl FnOnce(EventConsumer<CancellationMessage>) {
        move |stop_consumer| loop {
            if stop_consumer.is_cancelled(interval) {
                break;
            }

            // This relies on functionality present in `tracing_opentelemetry`'s `MetricsLayer`
            // <https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html#usage>,
            // `monotonic_counter` is used when the counter should only ever increase
            trace!(monotonic_counter.uptime = start_time.elapsed().as_secs_f64());
        }
    }
}

const DEFAULT_UPTIME_REPORTER_INTERVAL: Duration = Duration::from_secs(60);

/// The interval at which the uptime reporter emits an event.
#[derive(Deserialize, Debug, PartialEq, Clone, Copy, Serialize)]
pub struct UptimeReporterInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl Default for UptimeReporterInterval {
    fn default() -> Self {
        Self(DEFAULT_UPTIME_REPORTER_INTERVAL)
    }
}

impl From<Duration> for UptimeReporterInterval {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<UptimeReporterInterval> for Duration {
    fn from(value: UptimeReporterInterval) -> Self {
        value.0
    }
}
