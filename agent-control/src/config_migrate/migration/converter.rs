use crate::agent_type::agent_type_registry::AgentRepositoryError;
use crate::config_migrate::migration::{
    agent_value_spec::AgentValueError,
    config::{AgentTypeFieldFQN, DirInfo, MigrationAgentConfig},
};
use crate::sub_agent::effective_agents_assembler::AgentTypeDefinitionError;
use fs::LocalFile;
use fs::file_reader::{FileReader, FileReaderError};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    #[error("duplicate key found in file and dir mappings: {0}")]
    DuplicateKeyFound(AgentTypeFieldFQN),
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
        // Parse first file mappings (supposedly only a single infra-agent config)
        // then directory mappings (integrations and logs).
        // Both file and directory mappings are key-value structures. I assume
        // the keys are the intended variable names for the agent type, and the values
        // the places where the contents of these variables will be read from, namely
        // the files and directory paths respectively. They will be parsed to YAML or
        // key-value mappings (file as string, YAML) as appropriate.
        // Errors when reading and parsing files are ignored and not included in the
        // rendered values.

        let file_reader = &self.file_reader;

        let file_mapping_vars = migration_agent_config
            .files_map
            .iter()
            .map(|(k, v)| Ok((k, retrieve_file_mapping_value(file_reader, v)?)))
            .collect::<Result<HashMap<_, _>, ConversionError>>()?;

        let directory_mapping_vars = migration_agent_config
            .dirs_map
            .iter()
            .map(|(k, v)| Ok((k, retrieve_dir_mapping_values(file_reader, v)?)))
            .collect::<Result<HashMap<_, _>, ConversionError>>()?;

        // Search for duplicate keys and error out if found,
        // as duplicates would overwrite previous values silently
        // When transforming to the final YAML structure.
        let mut all_keys = file_mapping_vars
            .keys()
            .chain(directory_mapping_vars.keys())
            .copied();
        let mut visited = HashSet::new();
        all_keys.try_for_each(|k| {
            if !visited.insert(k) {
                Err(ConversionError::DuplicateKeyFound(k.clone()))
            } else {
                Ok(())
            }
        })?;

        todo!();
        Ok(HashMap::default())
    }
}

fn retrieve_file_mapping_value<F: FileReader>(
    file_reader: &F,
    file_path: &Path,
) -> Result<serde_yaml::Value, ConversionError> {
    let yaml_value = file_reader.read(file_path)?;
    let parsed_yaml: serde_yaml::Value = serde_yaml::from_str(&yaml_value)?;
    Ok(parsed_yaml)
}

fn retrieve_dir_mapping_values<F: FileReader>(
    file_reader: &F,
    dir_info: &DirInfo,
) -> Result<HashMap<PathBuf, serde_yaml::Value>, ConversionError> {
    let files = file_reader
        .dir_entries(&dir_info.path)?
        .into_iter()
        .filter(|p| dir_info.valid_filename(p));

    let mut read_files = files.map(|filepath| {
        file_reader
            .read(&filepath)
            .map(|content| (filepath, content))
    });

    read_files.try_fold(HashMap::new(), |mut acc, read_file| {
        let (filepath, content) = read_file?;
        let parsed = serde_yaml::from_str::<serde_yaml::Value>(&content)?;
        acc.insert(filepath, parsed);
        Ok(acc)
    })
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
