use std::collections::HashMap;
use regex::Regex;
use semver::Version;
use thiserror::Error;
use crate::crowdstrike::config::SensorOSMappingConfig;
use crate::crowdstrike::response::{Installer, SensorInstallers};

#[derive(Error, Debug)]
pub enum OSInstallerMapperError {
}


pub struct OSInstallerMapper {
    installer_os_mapping_config: SensorOSMappingConfig,
}

impl OSInstallerMapper {
    pub fn new(
        installer_os_mapping_config: SensorOSMappingConfig,
    ) -> Self {
        OSInstallerMapper {
            installer_os_mapping_config,
        }
    }

    pub fn get_latest_hashes_by_os(&self, sensor_installers: SensorInstallers) -> Result<HashMap<String, String>, OSInstallerMapperError> {
        let mut installers_by_os_version: HashMap<String, Vec<Installer>> = HashMap::new();

        for installer in sensor_installers.installers {
            let os_version_key = format!(
                "{}-{}",
                installer.os,
                installer.os_version,
            );
            if !installers_by_os_version.contains_key(&os_version_key) {
                installers_by_os_version.insert(os_version_key.clone(), vec![installer.clone()]);
                continue;
            }
            installers_by_os_version.get_mut(&os_version_key).unwrap().push(installer);
        }

        let mut os_hash_response: HashMap<String,String> = HashMap::new();
        for os_mapping in &self.installer_os_mapping_config.mapping {
            let os_version_key = format!(
                "{}-{}",
                os_mapping.1.os,
                os_mapping.1.os_version,
            );
            let re = Regex::new(os_mapping.1.name_pattern.as_str())
                .unwrap_or_else(|_| panic!("invalid filename_pattern: {}", os_mapping.1.name_pattern));

            if installers_by_os_version.contains_key(&os_version_key) {
                let mut version =  Version::parse("0.0.0").expect("default version correctly parsed");
                // TODO fix unwrap
                for installer in installers_by_os_version.get(&os_version_key).unwrap() {
                    if re.is_match(installer.name.as_str()) {
                        let current_version = Version::parse(installer.version.as_str()).unwrap_or(version.clone());
                        if current_version.gt(&version) {
                            os_hash_response.insert(os_mapping.0.clone(), installer.sha256.clone());
                            version = current_version.clone();
                        }
                    }
                }
            }
        }

        Ok(os_hash_response)
    }
}