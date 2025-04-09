use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_UPTIME_REPORT_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UptimeReportConfig {
    pub enabled: bool,
    pub interval: UptimeReportInterval,
}

impl Default for UptimeReportConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: UptimeReportInterval::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_UPTIME_REPORT_INTERVAL)]
pub struct UptimeReportInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);
