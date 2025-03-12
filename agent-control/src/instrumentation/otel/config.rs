use serde::{Deserialize, Serialize};
use std::fmt::Debug;

// TODO: move to instrumentation
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct OtelConfig {
    #[serde(default)]
    pub(crate) traces: bool,

    #[serde(default)]
    pub(crate) metrics: bool,
    // TODO: We should also have OTLP logs, and we should define if the config should be like this,
    // probably we need extra configuration for metrics and traces, like interval of sending or other.
}

impl OtelConfig {
    pub fn enabled(&self) -> bool {
        self.traces || self.metrics
    }
}
