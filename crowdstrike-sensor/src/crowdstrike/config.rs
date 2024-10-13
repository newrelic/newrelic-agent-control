use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum SensorOSMappingConfigError {
    #[error("error parsing yaml: `{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

#[derive(Deserialize)]
pub struct SensorOSMappingConfig {
    pub mapping: HashMap<String, Sensor>
}

#[derive(Deserialize)]
pub(super) struct Sensor {
    pub os: String,
    pub os_version: String,
    pub name_pattern: String
}
impl SensorOSMappingConfig {
    pub fn parse(config_content: &str) -> Result<Self, SensorOSMappingConfigError> {
        Ok(serde_yaml::from_str(config_content)?)
    }
}

#[cfg(test)]
mod test {
    use crate::crowdstrike::defaults::CROWDSTRIKE_SENSOR_INSTALLER_HASH_OS_MAPPING;
    use super::*;

    #[test]
    fn config_parse() {
        let config = SensorOSMappingConfig::parse(CROWDSTRIKE_SENSOR_INSTALLER_HASH_OS_MAPPING);
        assert!(config.is_ok())
    }
}