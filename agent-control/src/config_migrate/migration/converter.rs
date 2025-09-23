use crate::agent_type::agent_type_registry::AgentRepositoryError;
use crate::config_migrate::migration::agent_configs::SupportedConfigValue;
use crate::config_migrate::migration::agent_configs::SupportedConfigValueError;
use crate::config_migrate::migration::agent_value_spec::AgentValueError;
use crate::config_migrate::migration::config::MigrationAgentConfig;
use crate::config_migrate::migration::converter::ConversionError::*;
use crate::sub_agent::effective_agents_assembler::AgentTypeDefinitionError;
use fs::LocalFile;
use fs::file_reader::{FileReader, FileReaderError};
use std::collections::HashMap;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("{0}")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("{0}")]
    ConvertFileError(#[from] FileReaderError),
    #[error("{0}")]
    AgentValueError(#[from] AgentValueError),
    #[error("{0}")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("cannot find required file map: {0}")]
    RequiredFileMappingNotFoundError(String),
    #[error("cannot find required dir map: {0}")]
    RequiredDirMappingNotFoundError(String),
    #[error("deserializing YAML: {0}")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),
    #[error("retrieving supported config value: {0}")]
    SupportedConfigValueError(#[from] SupportedConfigValueError),
}

pub struct ConfigConverter<F: FileReader> {
    file_reader: F,
}

impl Default for ConfigConverter<LocalFile> {
    fn default() -> Self {
        ConfigConverter {
            file_reader: LocalFile,
        }
    }
}

#[cfg_attr(test, mockall::automock)]
impl<F: FileReader> ConfigConverter<F> {
    pub fn convert(
        &self,
        migration_agent_config: &MigrationAgentConfig,
    ) -> Result<HashMap<String, serde_yaml::Value>, ConversionError> {
        // Parse first config, then integrations and then logs.
        // We must know about the different types to be able to populate the variables correctly.
        // The [`MigrationAgentConfig`] structure can in theory support arbitrary agents and
        // thus we would first need to know if the sub-agent we are creating the values for is
        // supported in the first place, but we are not leveraging that currently as we only
        // (and probably forever) support migrating the on-host infrastructure-agent agent type.
        // Instead, we assume that certain fields are present and populate explicit structures
        // only for them. Not finding the expected structures is an error.

        let file_reader = &self.file_reader;

        // Retrieve the configuration for an existing infrastructure-agent installation.
        let config_agent_key = String::from("config_agent");
        let (k, v) = migration_agent_config
            .files_map
            .get_key_value(&config_agent_key.as_str().into())
            .ok_or(RequiredFileMappingNotFoundError(config_agent_key))?;
        let config_file = file_reader.read(v.as_path())?;
        let config_yaml = serde_yaml::from_str(&config_file)?;
        let infra_agent_config_spec = (k.as_string(), config_yaml);

        // Retrieve the configuration for existing infrastructure-agent integrations.d files.
        let config_integrations_key = String::from("config_integrations");
        let (k, v) = migration_agent_config
            .dirs_map
            .get_key_value(&config_integrations_key.as_str().into())
            .ok_or(RequiredDirMappingNotFoundError(config_integrations_key))?;

        let integrations_entries =
            SupportedConfigValue::from_dir_with_key(file_reader, v, "integrations")
                .map(serde_yaml::Value::from)?;
        let integrations_config_spec = (k.as_string(), integrations_entries);

        // Retrieve the configuration for existing infrastructure-agent logging.d files.
        let logging_key = String::from("config_logging");
        let (k, v) = migration_agent_config
            .dirs_map
            .get_key_value(&logging_key.as_str().into())
            .ok_or(RequiredDirMappingNotFoundError(logging_key))?;

        let logging_entries = SupportedConfigValue::from_dir_with_key(file_reader, v, "logs")
            .map(serde_yaml::Value::from)?;
        let logging_config_spec = (k.as_string(), logging_entries);

        Ok([
            infra_agent_config_spec,
            integrations_config_spec,
            logging_config_spec,
        ]
        .into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fs::mock::MockLocalFile;
    use mockall::{Sequence, predicate};

    use crate::config_migrate::migration::{
        agent_configs::tests::{EXAMPLE_INTEGRATION_CONFIG, EXAMPLE_LOGS_CONFIG},
        config::DirInfo,
    };

    use super::*;

    #[test]
    fn from_migration_config_to_conversion() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            files_map: HashMap::from([("config_agent".into(), "/etc/newrelic-infra.yml".into())]),
            dirs_map: HashMap::from([
                (
                    "config_integrations".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
            ]),
            next: None,
        };

        let config_agent = "license_key: TESTING_CONVERSION";

        let mut file_reader = MockLocalFile::new();

        // Capture in a sequence the three reads. First the config, then the integrations dir, then the logging dir.
        let mut sequence = Sequence::new();
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once({
                let config_agent = config_agent.to_string();
                move |_| Ok(config_agent)
            });

        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new(
                "/etc/newrelic-infra/integrations.d",
            )))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(vec![
                    PathBuf::from("integration1.yml"),
                    PathBuf::from("integration2.yaml"),
                ])
            });

        // Reading the two files "recovered" above for integrations.d
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("integration1.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
integrations:
  - name: nri-docker
    when:
      feature: docker_enabled
      file_exists: /var/run/docker.sock
    interval: 15s
"#,
                ))
            });

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("integration2.yaml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
integrations:
  - name: nri-docker
    when:
      feature: docker_enabled
      env_exists:
        FARGATE: "true"
    interval: 15s"#,
                ))
            });

        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new("/etc/newrelic-infra/logging.d")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(vec![
                    PathBuf::from("logging1.yaml"),
                    PathBuf::from("logging2.yml"),
                ])
            });

        // Reading the two files "recovered" above for logging.d
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging1.yaml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
logs:
  - name: basic-file
    file: /var/log/logFile.log
  - name: file-with-spaces-in-path
    file: /var/log/folder with spaces/logFile.log
  - name: file-with-attributes
    file: /var/log/logFile.log
    attributes:
      application: tomcat
      department: sales
      maintainer: example@mailprovider.com
"#,
                ))
            });

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging2.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
logs:
  - name: log-files-in-folder
    file: /var/log/logF*.log
  - name: log-file-with-long-lines
    file: /var/log/logFile.log
    max_line_kb: 256
  - name: only-records-with-warn-and-error
    file: /var/log/logFile.log
    pattern: WARN|ERROR
"#,
                ))
            });

        let config_converter = ConfigConverter { file_reader };

        let result = config_converter.convert(&migration_agent_config);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(3, result.len());
        assert!(result.contains_key("config_agent"));
        assert!(result.contains_key("config_integrations"));
        assert!(result.contains_key("config_logging"));

        let expected_config_agent =
            serde_yaml::from_str::<serde_yaml::Value>(config_agent).unwrap();
        assert_eq!(&expected_config_agent, result.get("config_agent").unwrap());

        let expected_integrations =
            serde_yaml::from_str::<serde_yaml::Value>(EXAMPLE_INTEGRATION_CONFIG).unwrap();
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );

        let expected_logs = serde_yaml::from_str::<serde_yaml::Value>(EXAMPLE_LOGS_CONFIG).unwrap();
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }

    #[test]
    fn empty_integrations_dir_entry() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            files_map: HashMap::from([("config_agent".into(), "/etc/newrelic-infra.yml".into())]),
            dirs_map: HashMap::from([
                (
                    "config_integrations".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
            ]),
            next: None,
        };

        let config_agent = "license_key: TESTING_CONVERSION";

        let mut file_reader = MockLocalFile::new();

        // Capture in a sequence the three reads. First the config, then the integrations dir, then the logging dir.
        let mut sequence = Sequence::new();
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once({
                let config_agent = config_agent.to_string();
                move |_| Ok(config_agent)
            });

        // Let's suppose the integrations.d directory is empty, so no files
        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new(
                "/etc/newrelic-infra/integrations.d",
            )))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| Ok(vec![]));

        // Continuing with logging.d
        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new("/etc/newrelic-infra/logging.d")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(vec![
                    PathBuf::from("logging1.yaml"),
                    PathBuf::from("logging2.yml"),
                ])
            });

        // Reading the two files "recovered" above for logging.d
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging1.yaml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
logs:
  - name: basic-file
    file: /var/log/logFile.log
  - name: file-with-spaces-in-path
    file: /var/log/folder with spaces/logFile.log
  - name: file-with-attributes
    file: /var/log/logFile.log
    attributes:
      application: tomcat
      department: sales
      maintainer: example@mailprovider.com
"#,
                ))
            });

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging2.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| {
                Ok(String::from(
                    r#"
logs:
  - name: log-files-in-folder
    file: /var/log/logF*.log
  - name: log-file-with-long-lines
    file: /var/log/logFile.log
    max_line_kb: 256
  - name: only-records-with-warn-and-error
    file: /var/log/logFile.log
    pattern: WARN|ERROR
"#,
                ))
            });

        let config_converter = ConfigConverter { file_reader };

        let result = config_converter.convert(&migration_agent_config);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(3, result.len());
        assert!(result.contains_key("config_agent"));
        assert!(result.contains_key("config_integrations"));
        assert!(result.contains_key("config_logging"));

        let expected_config_agent =
            serde_yaml::from_str::<serde_yaml::Value>(config_agent).unwrap();
        assert_eq!(&expected_config_agent, result.get("config_agent").unwrap());

        // Read integrations object should be present but empty array
        let expected_integrations =
            serde_yaml::from_str::<serde_yaml::Value>("integrations: []").unwrap();
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );

        let expected_logs = serde_yaml::from_str::<serde_yaml::Value>(EXAMPLE_LOGS_CONFIG).unwrap();
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }

    #[test]
    fn no_infra_agent_config_should_fail() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            files_map: HashMap::from([("config_agent".into(), "/etc/newrelic-infra.yml".into())]),
            dirs_map: HashMap::from([
                (
                    "config_integrations".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
            ]),
            next: None,
        };

        let mut file_reader = MockLocalFile::new();

        // Capture in a sequence the three reads. First the config, then the integrations dir, then the logging dir.
        let mut sequence = Sequence::new();
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(move |_| {
                Err(FileReaderError::FileNotFound(String::from(
                    "file not found: `/etc/newrelic-infra.yml`",
                )))
            });

        let config_converter = ConfigConverter { file_reader };

        let result = config_converter.convert(&migration_agent_config);
        assert!(result.is_err());
    }

    #[test]
    fn empty_integrations_and_logs_should_succeed() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            files_map: HashMap::from([("config_agent".into(), "/etc/newrelic-infra.yml".into())]),
            dirs_map: HashMap::from([
                (
                    "config_integrations".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    },
                ),
            ]),
            next: None,
        };

        let config_agent = "license_key: TESTING_CONVERSION";

        let mut file_reader = MockLocalFile::new();

        // Capture in a sequence the three reads. First the config, then the integrations dir, then the logging dir.
        let mut sequence = Sequence::new();
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once({
                let config_agent = config_agent.to_string();
                move |_| Ok(config_agent)
            });

        // Let's suppose the integrations.d directory is empty, so no files
        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new(
                "/etc/newrelic-infra/integrations.d",
            )))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| Ok(vec![]));

        // Continuing with logging.d
        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new("/etc/newrelic-infra/logging.d")))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_| Ok(vec![]));

        let config_converter = ConfigConverter { file_reader };

        let result = config_converter.convert(&migration_agent_config);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(3, result.len());
        assert!(result.contains_key("config_agent"));
        assert!(result.contains_key("config_integrations"));
        assert!(result.contains_key("config_logging"));

        let expected_config_agent =
            serde_yaml::from_str::<serde_yaml::Value>(config_agent).unwrap();
        assert_eq!(&expected_config_agent, result.get("config_agent").unwrap());

        // Read integrations object should be present but empty array
        let expected_integrations =
            serde_yaml::from_str::<serde_yaml::Value>("integrations: []").unwrap();
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );

        let expected_logs = serde_yaml::from_str::<serde_yaml::Value>("logs: []").unwrap();
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }
}
