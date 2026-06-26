//! Configuration for the agent type GUID checker (polling interval and initial delay).
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::time::Duration;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_GUID_CHECKER_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_GUID_CHECKER_INITIAL_DELAY: Duration = Duration::from_secs(30);
/// Initial delay used by Agent Control's own GUID checker (zero, since its HelmRelease is expected
/// to exist at startup).
pub const AGENT_CONTROL_GUID_CHECKER_INITIAL_DELAY: GuidCheckerInitialDelay =
    GuidCheckerInitialDelay(Duration::ZERO); // The Agent Control HelmRelease is supposed to exist when it starts.

/// The duration to wait between GUID checks.
#[derive(Debug, Clone, Deserialize, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_GUID_CHECKER_INTERVAL)]
pub struct GuidCheckerInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// The initial delay before the first GUID check is performed.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_GUID_CHECKER_INITIAL_DELAY)]
pub struct GuidCheckerInitialDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);
