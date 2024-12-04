use duration_str::deserialize_duration;
use serde::Deserialize;
use std::time::Duration;

const DEFAULT_VERSION_CHECKER_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_VERSION_CHECKER_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sVersionCheckerConfig {
    pub(crate) interval: VersionCheckerInterval,
}

#[derive(Debug, Clone, Deserialize, Copy, PartialEq)]
pub struct VersionCheckerInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl From<VersionCheckerInterval> for Duration {
    fn from(value: VersionCheckerInterval) -> Self {
        value.0
    }
}
impl From<Duration> for VersionCheckerInterval {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl Default for VersionCheckerInterval {
    fn default() -> Self {
        Self(DEFAULT_VERSION_CHECKER_INTERVAL)
    }
}

#[derive(Debug, Clone, Deserialize, Copy, PartialEq)]
pub struct VersionCheckerTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl From<VersionCheckerTimeout> for Duration {
    fn from(value: VersionCheckerTimeout) -> Self {
        value.0
    }
}
impl From<Duration> for VersionCheckerTimeout {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl Default for VersionCheckerTimeout {
    fn default() -> Self {
        Self(DEFAULT_VERSION_CHECKER_TIMEOUT)
    }
}
