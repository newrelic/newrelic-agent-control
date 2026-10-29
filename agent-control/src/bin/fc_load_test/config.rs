//! Load-test configuration file.
use duration_str::deserialize_duration;
use newrelic_agent_control::agent_control::config::AgentControlConfig;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_HEALTH_INTERVAL: Duration = Duration::from_secs(60);

/// Error loading the load-test configuration.
#[derive(Debug, thiserror::Error)]
#[error("invalid load-test config: {0}")]
pub struct ConfigError(String);

/// Load-test configuration, see `fc-load-test.example.yaml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadTestConfig {
    /// Number of simulated Agent Controls.
    pub count: usize,
    /// Simulated Agent Controls started per second.
    pub ramp_up_per_second: u32,
    /// Directory holding one data directory per simulated Agent Control.
    pub data_root: PathBuf,
    /// Prefix of every simulated host name; changing it yields new identities.
    pub run_prefix: String,
    /// Wipe the data of this process's indices before starting.
    #[serde(default)]
    pub fresh: bool,
    /// Global tracing filter.
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Index of the first simulated Agent Control.
    #[serde(default)]
    pub index_offset: usize,
    /// Interval of the fake file health check.
    #[serde(
        default = "default_health_interval",
        deserialize_with = "deserialize_duration"
    )]
    pub health_interval: Duration,
    /// Extra agent type ids to shadow on top of the embedded ones.
    #[serde(default)]
    pub fake_agent_types: Vec<FakeAgentTypeId>,
    /// Agent Control config template, written for every simulated Agent Control.
    pub agent_control: serde_json::Value,
}

/// Identifier of an extra agent type to shadow.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FakeAgentTypeId {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

fn default_log_level() -> String {
    DEFAULT_LOG_LEVEL.to_string()
}

fn default_health_interval() -> Duration {
    DEFAULT_HEALTH_INTERVAL
}

impl LoadTestConfig {
    /// Reads and validates the configuration file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|err| ConfigError(format!("reading {}: {err}", path.display())))?;
        Self::parse(&content)
    }

    fn parse(content: &str) -> Result<Self, ConfigError> {
        let config: Self =
            serde_saphyr::from_str(content).map_err(|err| ConfigError(err.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.count == 0 {
            return Err(ConfigError("count must be greater than 0".to_string()));
        }
        if self.ramp_up_per_second == 0 {
            return Err(ConfigError(
                "ramp_up_per_second must be greater than 0".to_string(),
            ));
        }
        let prefix_is_safe = !self.run_prefix.is_empty()
            && self
                .run_prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !prefix_is_safe {
            return Err(ConfigError(
                "run_prefix must be non-empty and only contain [A-Za-z0-9_-]".to_string(),
            ));
        }
        let template = self.template()?;
        if template.fleet_control.is_none() {
            return Err(ConfigError(
                "agent_control.fleet_control is required".to_string(),
            ));
        }
        Ok(())
    }

    /// Parses the Agent Control template.
    pub fn template(&self) -> Result<AgentControlConfig, ConfigError> {
        serde_json::from_value(self.agent_control.clone())
            .map_err(|err| ConfigError(format!("agent_control: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fc-load-test.example.yaml");
        LoadTestConfig::load(&path).unwrap();
    }
}
