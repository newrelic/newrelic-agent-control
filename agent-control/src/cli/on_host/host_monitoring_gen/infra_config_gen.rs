use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::defaults::{AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR};
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
use crate::on_host::file_store::FileStore;
use crate::values::ConfigRepo;
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

const INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml";

impl Default for InfraConfigGenerator {
    fn default() -> Self {
        Self {
            local_dir: PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR),
            remote_dir: PathBuf::from(AGENT_CONTROL_DATA_DIR),
            config_mapping: NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING.to_string(),
            infra_config_path: Path::new(INFRA_CONFIG_PATH).to_path_buf(),
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
        let file_store = Arc::new(FileStore::new_local_fs(
            self.local_dir.clone(),
            self.remote_dir.clone(),
        ));
        let vr = ConfigRepo::new(file_store);
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
            let infra_values_persister = ValuesPersisterFile::new(self.local_dir.clone());
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
        info!("Migrating old infra agent configuration");
        let modified_yaml =
            infra_config.generate_agent_type_config_mapping(self.config_mapping.as_str())?;

        let config = MigrationConfig::parse(modified_yaml.as_str())
            .map_err(|err| CliError::Command(format!("error parsing migration config: {err}")))?;

        let file_store = Arc::new(FileStore::new_local_fs(
            self.local_dir.clone(),
            self.remote_dir.clone(),
        ));
        let vr = ConfigRepo::new(file_store);
        let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(vr));
        let config_migrator = ConfigMigrator::new(
            ConfigConverter::default(),
            AgentConfigGetter::new(sa_local_config_loader),
            ValuesPersisterFile::new(self.local_dir.clone()),
        );

        let legacy_config_renamer = LegacyConfigRenamer::default();

        for cfg in config.configs {
            let fqn = cfg.agent_type_fqn.clone();
            debug!("Checking configurations for {fqn}");
            let migrate_result = config_migrator.migrate(&cfg);
            if let Err(MigratorError::AgentTypeNotFoundOnConfig) = migrate_result {
                debug!("No agents of agent_type {fqn} found on config, skipping",);
                continue;
            };

            if let Err(e) = migrate_result {
                warn!("Could not apply local config migration for {fqn}: {e}",);
                continue;
            };

            for (_, mapping_type) in cfg.filesystem_mappings {
                let (mapping_key, mapping_path) = match mapping_type {
                    MappingType::Dir(dir_path) => ("path", dir_path.dir_path),
                    MappingType::File(file_info) => ("file", file_info.file_path),
                };
                legacy_config_renamer
                    .rename_path(mapping_path.as_path())
                    .map_err(|err| {
                        CliError::Command(format!(
                            "error renaming {mapping_key} on migration: {err}"
                        ))
                    })?;
            }
            debug!("Classic config files and paths renamed");
        }
        info!("Local config files successfully created");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::defaults::{
        AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    };
    use std::fs;
    use std::fs::create_dir_all;
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

    const INFRA_AGENT_VALUES: &str = "local-data/infra-test/local_config.yaml";

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added (DirectoryManager unimplemented)
    #[test]
    fn test_migrate_old_infra_config() {
        // Create a temporary directory

        use crate::on_host::file_store::build_config_name;
        let temp_dir = TempDir::new().unwrap();
        let infra_file_path = temp_dir.path().join("newrelic-infra.yml");
        let agents_file_path = temp_dir
            .path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID);
        create_dir_all(&agents_file_path).unwrap();
        // Emulate the existence of the file by creating it
        fs::write(&infra_file_path, INITIAL_INFRA_CONFIG).unwrap();

        fs::write(
            agents_file_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
            AGENTS_CONFIG,
        )
        .unwrap();

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
        let values_content = fs::read_to_string(&values_file).unwrap();

        let parsed_values: serde_yaml::Value = serde_yaml::from_str(&values_content).unwrap();

        let expected = r"#
config_agent:
  extra_config: true
  status_server_enabled: true
  enable_process_metrics: true
  license_key: '{{NEW_RELIC_LICENSE_KEY}}'
  status_server_port: 18003
  staging: true
  proxy: https://new-proxy.com
#";
        let expected_values: serde_yaml::Value = serde_yaml::from_str(expected).unwrap();
        assert_eq!(parsed_values, expected_values);
    }

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added (DirectoryManager unimplemented)
    #[test]
    fn test_generate_new_infra_config() {
        use crate::on_host::file_store::build_config_name;

        let temp_dir = TempDir::new().unwrap();
        let agents_file_path = temp_dir
            .path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID);
        create_dir_all(&agents_file_path).unwrap();

        fs::write(
            agents_file_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
            AGENTS_CONFIG,
        )
        .unwrap();

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

        let expected = r"#
config_agent:
  status_server_enabled: true
  enable_process_metrics: true
  license_key: '{{NEW_RELIC_LICENSE_KEY}}'
  status_server_port: 18003
#";
        let expected_values: serde_yaml::Value = serde_yaml::from_str(expected).unwrap();
        assert_eq!(parsed_values, expected_values);
    }
}
