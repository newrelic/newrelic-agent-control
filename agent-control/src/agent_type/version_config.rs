use duration_str::deserialize_duration;
use serde::Deserialize;
use std::time::Duration;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_VERSION_CHECKER_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sVersionCheckerConfig {
    pub(crate) interval: VersionCheckerInterval,
}

#[derive(Debug, Clone, Deserialize, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_VERSION_CHECKER_INTERVAL)]
pub struct VersionCheckerInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);
