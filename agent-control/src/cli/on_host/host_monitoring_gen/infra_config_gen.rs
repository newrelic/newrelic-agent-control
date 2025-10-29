use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, SUB_AGENT_DIR,
};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::region::Region;
use crate::cli::on_host::host_monitoring_gen::infra_config::{
    INFRA_AGENT_TYPE_VERSION, InfraConfig,
};
use crate::config_migrate::migration::agent_config_getter::AgentConfigGetter;
use crate::config_migrate::migration::config::{MappingType, MigrationConfig};
use crate::config_migrate::migration::converter::ConfigConverter;
use crate::config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
use crate::config_migrate::migration::migrator::{ConfigMigrator, MigratorError};
use crate::config_migrate::migration::persister::legacy_config_renamer::LegacyConfigRenamer;
use crate::config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use crate::values::file::ConfigRepositoryFile;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// InfraConfigGenerator is capable of migrating an actual infra-agent config into ac values format,
/// if no infra-agent config is found it generated the values from scratch.
pub struct InfraConfigGenerator {
    local_dir: PathBuf,
    remote_dir: PathBuf,
    config_mapping: String,
    infra_config_path: PathBuf,
}

impl Default for InfraConfigGenerator {
    fn default() -> Self {
        Self {
            local_dir: PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR),
            remote_dir: PathBuf::from(AGENT_CONTROL_DATA_DIR),
            config_mapping: NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING.to_string(),
            infra_config_path: Path::new("/etc/newrelic-infra.yml").to_path_buf(),
        }
    }
}

impl InfraConfigGenerator {
    pub fn new(
        local_dir: PathBuf,
        remote_dir: PathBuf,
        config_mapping: String,
        infra_config_path: PathBuf,
    ) -> Self {
        Self {
            local_dir,
            remote_dir,
            config_mapping,
            infra_config_path,
        }
    }
    pub fn generate_infra_config(
        &self,
        region: Region,
        custom_attributes: Option<String>,
        proxy: Option<String>,
    ) -> Result<(), CliError> {
        info!("Generating infra agent configuration");

        let mut infra_config = InfraConfig::default().with_region(region);
        if let Some(ca) = custom_attributes {
            infra_config = infra_config.with_custom_attributes(ca.as_str())?;
        }
        if let Some(px) = proxy {
            infra_config = infra_config.with_proxy(px.as_str());
        }

        if self.infra_config_path.is_file() {
            return self.migrate_old_infra(infra_config);
        }

        self.create_new_infra_values(infra_config)
    }

    fn create_new_infra_values(&self, infra_config: InfraConfig) -> Result<(), CliError> {
        info!("Creating new infra agent configuration");
        let vr = ConfigRepositoryFile::new(self.local_dir.clone(), self.remote_dir.clone());
        let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(vr));

        let config_getter = AgentConfigGetter::new(sa_local_config_loader);
        let agent_type_id = AgentTypeID::try_from(INFRA_AGENT_TYPE_VERSION).map_err(|err| {
            CliError::Command(format!("error on agent type for infra values: {err}"))
        })?;
        let Ok(sub_agents_cfg) =
            config_getter.get_agents_of_type_between_versions(agent_type_id, None)
        else {
            return Err(CliError::Command(
                "Agent type not found on config".to_string(),
            ));
        };

        for (agent_id, _) in sub_agents_cfg.agents {
            let infra_values_persister =
                ValuesPersisterFile::new(self.local_dir.join(SUB_AGENT_DIR));
            infra_values_persister
                .persist_values_file(
                    &agent_id,
                    infra_config.generate_infra_config_values()?.as_str(),
                )
                .map_err(|err| {
                    CliError::Command(format!("error persisting infra values: {err}"))
                })?;
        }
        info!("Local config files successfully created");

        Ok(())
    }

    fn migrate_old_infra(&self, infra_config: InfraConfig) -> Result<(), CliError> {
        info!("migrating old infra agent configuration");
        let modified_yaml =
            infra_config.generate_agent_type_config_mapping(self.config_mapping.as_str())?;

        let config = MigrationConfig::parse(modified_yaml.as_str())
            .map_err(|err| CliError::Command(format!("error parsing migration config: {err}")))?;

        let vr = ConfigRepositoryFile::new(self.local_dir.clone(), self.remote_dir.clone());
        let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(vr));
        let config_migrator = ConfigMigrator::new(
            ConfigConverter::default(),
            AgentConfigGetter::new(sa_local_config_loader),
            ValuesPersisterFile::new(self.local_dir.join(SUB_AGENT_DIR)),
        );

        let legacy_config_renamer = LegacyConfigRenamer::default();

        for cfg in config.configs {
            debug!("Checking configurations for {}", cfg.agent_type_fqn);
            match config_migrator.migrate(&cfg) {
                Ok(_) => {
                    for (_, mapping_type) in cfg.filesystem_mappings {
                        match mapping_type {
                            MappingType::Dir(dir_path) => {
                                legacy_config_renamer
                                    .rename_path(dir_path.dir_path.as_path())
                                    .map_err(|err| {
                                        CliError::Command(format!(
                                            "error renaming path on migration: {err}"
                                        ))
                                    })?;
                            }
                            MappingType::File(file_info) => {
                                legacy_config_renamer
                                    .rename_path(file_info.file_path.as_path())
                                    .map_err(|err| {
                                        CliError::Command(format!(
                                            "error renaming file on migration: {err}"
                                        ))
                                    })?;
                            }
                        }
                    }
                    debug!("Classic config files and paths renamed");
                }
                Err(MigratorError::AgentTypeNotFoundOnConfig) => {
                    debug!(
                        "No agents of agent_type {} found on config, skipping",
                        cfg.agent_type_fqn.clone()
                    );
                }
                Err(e) => {
                    warn!(
                        "Could not apply local config migration for {}: {}",
                        cfg.agent_type_fqn, e
                    );
                }
            }
        }
        info!("Local config files successfully created");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const INITIAL_INFRA_CONFIG: &str = r#"
license_key: invented
enable_process_metrics: false
status_server_enabled: false
status_server_port: 2333
extra_config: true
proxy: https://old-proxy.com
"#;

    const AGENTS_CONFIG: &str = r#"
agents:
    infra-test:
        agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#;

    const INFRA_AGENT_VALUES: &str = "fleet/agents.d/infra-test/values/values.yaml";

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added
    #[test]
    fn test_migrate_old_infra_config() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let infra_file_path = temp_dir.path().join("newrelic-infra.yml");
        let agents_file_path = temp_dir.path().join("config.yaml");

        // Emulate the existence of the file by creating it
        fs::write(&infra_file_path, INITIAL_INFRA_CONFIG).unwrap();

        fs::write(&agents_file_path, AGENTS_CONFIG).unwrap();

        // Format the string using dynamic file path
        let config_mapping = format!(
            r#"
configs:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure:0.1.0
    filesystem_mappings:
      config_agent:
        file_path: {}
        overwrites: {{}}
        deletions: []
"#,
            infra_file_path.to_str().unwrap()
        );

        let new_proxy = "https://new-proxy.com".to_string();

        let infra_config_generator = InfraConfigGenerator::new(
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
            config_mapping,
            infra_file_path,
        );

        let result = infra_config_generator.generate_infra_config(
            Region::STAGING,
            None,
            Some(new_proxy.clone()),
        );
        assert!(result.is_ok());

        let values_file = temp_dir.path().join(INFRA_AGENT_VALUES);

        // Read the contents of the values.yaml file
        let values_content = fs::read_to_string(&values_file).unwrap();

        // Parse the YAML content
        let parsed_values: serde_yaml::Value = serde_yaml::from_str(&values_content).unwrap();

        // Assertions to verify the contents of the values.yaml file
        if let serde_yaml::Value::Mapping(map) = parsed_values {
            if let Some(serde_yaml::Value::Mapping(config_agent_map)) =
                map.get(serde_yaml::Value::String("config_agent".to_string()))
            {
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("staging".to_string())),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("extra_config".to_string())),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "status_server_enabled".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "enable_process_metrics".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("license_key".to_string())),
                    Some(&serde_yaml::Value::String(
                        "{{NEW_RELIC_LICENSE_KEY}}".to_string()
                    ))
                );
                assert_eq!(
                    config_agent_map
                        .get(serde_yaml::Value::String("status_server_port".to_string())),
                    Some(&serde_yaml::Value::Number(serde_yaml::Number::from(18003)))
                );
                // proxy is modified
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("proxy".to_string())),
                    Some(&serde_yaml::Value::String(new_proxy))
                );
            }
        } else {
            panic!("Expected a YAML mapping");
        }
    }

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added
    #[test]
    fn test_generate_new_infra_config() {
        let temp_dir = TempDir::new().unwrap();
        let agents_file_path = temp_dir.path().join("config.yaml");

        fs::write(&agents_file_path, AGENTS_CONFIG).unwrap();

        let infra_config_generator = InfraConfigGenerator::new(
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
            NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING.to_string(),
            Path::new("/an/invented/path").to_path_buf(),
        );

        let _ = infra_config_generator.generate_infra_config(Region::US, None, None);

        let infra_config = InfraConfig::default().with_custom_attributes("").unwrap();
        let result = infra_config_generator.create_new_infra_values(infra_config);
        assert!(result.is_ok());

        let values_file = temp_dir.path().join(INFRA_AGENT_VALUES);

        // Read the contents of the values.yaml file
        let values_content = fs::read_to_string(&values_file).unwrap();

        // Parse the YAML content
        let parsed_values: serde_yaml::Value = serde_yaml::from_str(&values_content).unwrap();

        // Assertions to verify the contents of the values.yaml file
        if let serde_yaml::Value::Mapping(map) = parsed_values {
            if let Some(serde_yaml::Value::Mapping(config_agent_map)) =
                map.get(serde_yaml::Value::String("config_agent".to_string()))
            {
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "status_server_enabled".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "enable_process_metrics".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("license_key".to_string())),
                    Some(&serde_yaml::Value::String(
                        "{{NEW_RELIC_LICENSE_KEY}}".to_string()
                    ))
                );
                assert_eq!(
                    config_agent_map
                        .get(serde_yaml::Value::String("status_server_port".to_string())),
                    Some(&serde_yaml::Value::Number(serde_yaml::Number::from(18003)))
                );
            }
        } else {
            panic!("Expected a YAML mapping");
        }
    }
}
