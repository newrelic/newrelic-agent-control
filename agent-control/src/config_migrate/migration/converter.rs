use crate::agent_type::agent_type_registry::AgentRepositoryError;
use crate::config_migrate::migration::config::{FileInfo, MappingType};
use crate::config_migrate::migration::{
    agent_value_spec::AgentValueError,
    config::{DirInfo, MigrationAgentConfig},
};
use crate::sub_agent::effective_agents_assembler::AgentTypeDefinitionError;
use fs::LocalFile;
use fs::file_reader::{FileReader, FileReaderError};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;
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

        let file_reader = &self.file_reader;

        migration_agent_config
            .filesystem_mappings
            .iter()
            .map(|(k, v)| match v {
                MappingType::File(file_info) => Ok((
                    k.to_string(),
                    retrieve_file_mapping_value(file_reader, file_info)?,
                )),
                MappingType::Dir(dir_info) => Ok((
                    k.to_string(),
                    retrieve_dir_mapping_values(file_reader, dir_info)?,
                )),
            })
            .collect::<Result<HashMap<_, _>, ConversionError>>()
    }
}

fn retrieve_file_mapping_value<F: FileReader>(
    file_reader: &F,
    file_info: &FileInfo,
) -> Result<serde_yaml::Value, ConversionError> {
    let yaml_value = file_reader.read(file_info.file_path.as_path())?;
    let mut parsed_yaml: serde_yaml::Value = serde_yaml::from_str(&yaml_value)?;
    // Overwrite or add attributes from the HashMap
    if let serde_yaml::Value::Mapping(ref mut map) = parsed_yaml {
        // Remove elements based on the Vec of keys
        for key in &file_info.deletions {
            map.remove(serde_yaml::Value::String(key.clone()));
        }

        // Add or overwrite elements based on the overwrites hashmap
        for (key, value) in &file_info.overwrites {
            map.insert(serde_yaml::Value::String(key.clone()), value.clone());
        }
    }

    Ok(parsed_yaml)
}

fn retrieve_dir_mapping_values<F: FileReader>(
    file_reader: &F,
    dir_info: &DirInfo,
) -> Result<serde_yaml::Value, ConversionError> {
    let valid_extension_files = file_reader
        .dir_entries(&dir_info.dir_path)?
        .into_iter()
        .filter(|p| dir_info.valid_filename(p));

    let mut read_files = valid_extension_files.map(|filepath| {
        file_reader.read(&filepath).map(|content| {
            // If I am here means read was successful (it was a file), so I can unwrap `file_name`.
            let filename = filepath.file_name().unwrap().to_string_lossy().to_string();
            (filename, content)
        })
    });

    let read_files = read_files.try_fold(HashMap::new(), |mut acc, read_file| {
        let (filepath, content) = read_file?;
        let parsed = serde_yaml::from_str::<serde_yaml::Value>(&process_config_input(content))?;
        acc.insert(filepath, parsed);
        Ok::<_, ConversionError>(acc)
    })?;

    Ok(serde_yaml::to_value(read_files)?)
}

/// Handles the usage of environment variables in the YAML config files via the special
/// `{{VAR_NAME}}` syntax, by replacing them with a YAML-compatible syntax `'{{VAR_NAME}}'`.
/// (just adding quotes to make it a string). If this pattern is not quoted, the resulting YAML
/// would evaluate to a nested mapping with a single key-null pair, which is not what we want.
///
/// This uses a regex-based approach.
fn process_config_input(input: String) -> String {
    env_var_syntax_regex()
        .replace_all(&input, "${pre}'{{${2}}}'${post}")
        .to_string()
}

/// Regex to match {{VAR_NAME}} if not already inside quotes. Used for pre-processing YAML configs
/// coming from the infrastructure-agent which may include this syntax for env var interpolation.
///
/// The Regex is compiled just once and reused.
fn env_var_syntax_regex() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| {
        Regex::new(r#"(?P<pre>[^'"]|^)\{\{([A-Za-z0-9_]+)\}\}(?P<post>[^'"]|$)"#).unwrap()
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fs::mock::MockLocalFile;
    use mockall::{PredicateBooleanExt, predicate};
    use rstest::rstest;

    use crate::config_migrate::migration::config::DirInfo;

    use super::*;

    const INTEGRATION_1_CONFIG: &str = r#"
integrations:
  - name: nri-docker
    when:
      feature: docker_enabled
      file_exists: /var/run/docker.sock
    interval: 15s
"#;

    const INTEGRATION_2_CONFIG: &str = r#"
integrations:
  - name: nri-docker
    when:
      feature: docker_enabled
      env_exists:
        FARGATE: "true"
    interval: 15s
"#;

    const LOGS_1_CONFIG: &str = r#"
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
"#;

    const LOGS_2_CONFIG: &str = r#"
logs:
  - name: log-files-in-folder
    file: /var/log/logF*.log
  - name: log-file-with-long-lines
    file: /var/log/logFile.log
    max_line_kb: 256
  - name: only-records-with-warn-and-error
    file: /var/log/logFile.log
    pattern: WARN|ERROR
"#;

    #[rstest]
    #[case::no_templates("license_key: {{MY_ENV_VAR}}", "license_key: '{{MY_ENV_VAR}}'")]
    #[case::multiple_templates(
        "license_key: {{MY_ENV_VAR}} other {{ANOTHER_ENV}}",
        "license_key: '{{MY_ENV_VAR}}' other '{{ANOTHER_ENV}}'"
    )]
    #[case::no_templates_at_all(
        "license_key: my_real_license_key",
        "license_key: my_real_license_key"
    )]
    #[case::multiline_yaml_syntax(
        "license_key: {{MY_ENV_VAR}}\nother_key: value",
        "license_key: '{{MY_ENV_VAR}}'\nother_key: value"
    )]
    #[case::already_quoted("license_key: '{{MY_ENV_VAR}}'", "license_key: '{{MY_ENV_VAR}}'")]
    #[case::double_quoted(r#"license_key: "{{MY_ENV_VAR}}""#, "license_key: \"{{MY_ENV_VAR}}\"")]
    fn env_var_interpolation(#[case] input: &str, #[case] output: &str) {
        let result = process_config_input(input.to_string());
        assert_eq!(result, output);
    }

    #[test]
    fn from_migration_config_to_conversion() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            filesystem_mappings: HashMap::from([
                (
                    "config_agent".into(),
                    FileInfo {
                        file_path: "/etc/newrelic-infra.yml".into(),
                        overwrites: HashMap::default(),
                        deletions: Vec::default(),
                    }
                    .into(),
                ),
                (
                    "config_integrations".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
            ]),
            next: None,
        };

        let config_agent = r#"
license_key: TESTING_CONVERSION
element: dsfadsf
"#;

        let mut file_reader = MockLocalFile::new();

        // Capture in a sequence the three reads. First the config, then the integrations dir, then the logging dir.
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
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
            .return_once(|_| Ok(String::from(INTEGRATION_1_CONFIG)));

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("integration2.yaml")))
            .times(1)
            .return_once(|_| Ok(String::from(INTEGRATION_2_CONFIG)));

        file_reader
            .expect_dir_entries()
            .with(predicate::eq(Path::new("/etc/newrelic-infra/logging.d")))
            .times(1)
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
            .return_once(|_| Ok(String::from(LOGS_1_CONFIG)));

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging2.yml")))
            .times(1)
            .return_once(|_| Ok(String::from(LOGS_2_CONFIG)));

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

        let mut expected_integrations_mapping = serde_yaml::Mapping::new();
        expected_integrations_mapping.insert(
            serde_yaml::Value::String("integration1.yml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(INTEGRATION_1_CONFIG).unwrap(),
        );
        expected_integrations_mapping.insert(
            serde_yaml::Value::String("integration2.yaml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(INTEGRATION_2_CONFIG).unwrap(),
        );
        let expected_integrations = serde_yaml::Value::Mapping(expected_integrations_mapping);
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );

        let mut expected_logs_mapping = serde_yaml::Mapping::new();
        expected_logs_mapping.insert(
            serde_yaml::Value::String("logging1.yaml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(LOGS_1_CONFIG).unwrap(),
        );
        expected_logs_mapping.insert(
            serde_yaml::Value::String("logging2.yml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(LOGS_2_CONFIG).unwrap(),
        );
        let expected_logs = serde_yaml::Value::Mapping(expected_logs_mapping);
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }

    #[test]
    fn empty_integrations_dir_entry() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            filesystem_mappings: HashMap::from([
                (
                    "config_agent".into(),
                    FileInfo {
                        file_path: "/etc/newrelic-infra.yml".into(),
                        overwrites: HashMap::default(),
                        deletions: Vec::default(),
                    }
                    .into(),
                ),
                (
                    "config_integrations".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
            ]),
            next: None,
        };

        let config_agent = "license_key: TESTING_CONVERSION";

        let mut file_reader = MockLocalFile::new();

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .return_once({
                let config_agent = config_agent.to_string();
                move |_| Ok(config_agent)
            });

        // Let's suppose the integrations.d directory is empty, so no files
        file_reader
            .expect_dir_entries()
            .with(
                predicate::eq(Path::new("/etc/newrelic-infra/logging.d")).or(predicate::eq(
                    Path::new("/etc/newrelic-infra/integrations.d"),
                )),
            )
            .times(2)
            .returning(|dir| {
                let output = if dir == Path::new("/etc/newrelic-infra/logging.d") {
                    vec![
                        PathBuf::from("logging1.yaml"),
                        PathBuf::from("logging2.yml"),
                    ]
                } else {
                    vec![]
                };
                Ok(output)
            });

        // Reading the two files "recovered" above for logging.d
        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging1.yaml")))
            .times(1)
            .return_once(|_| Ok(String::from(LOGS_1_CONFIG)));

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("logging2.yml")))
            .times(1)
            .return_once(|_| Ok(String::from(LOGS_2_CONFIG)));

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
        let expected_integrations = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );
        let mut expected_logs_mapping = serde_yaml::Mapping::new();
        expected_logs_mapping.insert(
            serde_yaml::Value::String("logging1.yaml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(LOGS_1_CONFIG).unwrap(),
        );
        expected_logs_mapping.insert(
            serde_yaml::Value::String("logging2.yml".into()),
            serde_yaml::from_str::<serde_yaml::Value>(LOGS_2_CONFIG).unwrap(),
        );
        let expected_logs = serde_yaml::Value::Mapping(expected_logs_mapping);
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }

    #[test]
    fn no_infra_agent_config_should_fail() {
        // Sample config
        let migration_agent_config = MigrationAgentConfig {
            agent_type_fqn: "newrelic/com.newrelic.infrastructure:0.1.0"
                .try_into()
                .unwrap(),
            filesystem_mappings: HashMap::from([
                (
                    "config_agent".into(),
                    FileInfo {
                        file_path: "/etc/newrelic-infra.yml".into(),
                        overwrites: HashMap::default(),
                        deletions: Vec::default(),
                    }
                    .into(),
                ),
                (
                    "config_integrations".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
            ]),
            next: None,
        };

        let mut file_reader = MockLocalFile::new();

        file_reader
            .expect_dir_entries()
            .with(predicate::always())
            // We don't care about the dir entries for this test
            .returning(|_| Ok(vec![]));
        file_reader
            .expect_read()
            .with(predicate::always())
            .return_once(move |p| {
                if p == Path::new("/etc/newrelic-infra.yml") {
                    Err(FileReaderError::FileNotFound(String::from(
                        "file not found: `/etc/newrelic-infra.yml`",
                    )))
                } else {
                    // Default string because we don't care about other reads
                    Ok(String::new())
                }
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
            filesystem_mappings: HashMap::from([
                (
                    "config_agent".into(),
                    FileInfo {
                        file_path: "/etc/newrelic-infra.yml".into(),
                        overwrites: HashMap::default(),
                        deletions: Vec::default(),
                    }
                    .into(),
                ),
                (
                    "config_integrations".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/integrations.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
                (
                    "config_logging".into(),
                    DirInfo {
                        dir_path: "/etc/newrelic-infra/logging.d".into(),
                        extensions: vec!["yml".to_string(), "yaml".to_string()],
                    }
                    .into(),
                ),
            ]),
            next: None,
        };

        let config_agent = "license_key: TESTING_CONVERSION";

        let mut file_reader = MockLocalFile::new();

        file_reader
            .expect_read()
            .with(predicate::eq(Path::new("/etc/newrelic-infra.yml")))
            .times(1)
            .return_once({
                let config_agent = config_agent.to_string();
                move |_| Ok(config_agent)
            });

        // Let's suppose both integrations.d and logging.d directories are empty, so no files
        file_reader
            .expect_dir_entries()
            .with(
                predicate::eq(Path::new("/etc/newrelic-infra/logging.d")).or(predicate::eq(
                    Path::new("/etc/newrelic-infra/integrations.d"),
                )),
            )
            .times(2)
            .returning(|_| Ok(vec![]));

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
        let expected_integrations = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        assert_eq!(
            &expected_integrations,
            result.get("config_integrations").unwrap()
        );

        let expected_logs = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        assert_eq!(&expected_logs, result.get("config_logging").unwrap());
    }

    #[test]
    fn test_retrieve_file_mapping_value() {
        // Mock YAML content
        let yaml_content = r#"
        license_key: "old_key"
        status_server_port: 8080
        status_server_enabled: true
        enable_process_metrics: false
        extra_field: true
        staging: "test"
        custom_attributes: {}
        is_integrations_only: false
        "#;

        let mut file_reader = MockLocalFile::new();

        file_reader
            .expect_read()
            .times(1)
            .returning(|_| Ok(yaml_content.to_string()));

        // Create a FileInfo instance with overwrites and deletions
        let file_info = FileInfo {
            file_path: PathBuf::from("/a/path/newrelic-infra.yml"),
            overwrites: {
                let mut map = HashMap::new();
                map.insert(
                    "license_key".to_string(),
                    serde_yaml::Value::String("new_key".to_string()),
                );
                map.insert(
                    "status_server_port".to_string(),
                    serde_yaml::Value::Number(serde_yaml::Number::from(9090)),
                );
                map.insert(
                    "status_server_enabled".to_string(),
                    serde_yaml::Value::Bool(false),
                );
                map.insert(
                    "enable_process_metrics".to_string(),
                    serde_yaml::Value::Bool(true),
                );
                map
            },
            deletions: vec![
                "staging".to_string(),
                "enable_process_metrics".to_string(),
                "status_server_enabled".to_string(),
                "status_server_port".to_string(),
                "license_key".to_string(),
                "custom_attributes".to_string(),
                "is_integrations_only".to_string(),
            ],
        };

        // Call the function
        let result = retrieve_file_mapping_value(&file_reader, &file_info);

        // Assert the result
        assert!(result.is_ok());
        let parsed_yaml = result.unwrap();

        // Check that the YAML has been modified correctly
        if let serde_yaml::Value::Mapping(map) = parsed_yaml {
            assert!(
                map.get(serde_yaml::Value::String(
                    "enable_process_metrics".to_string()
                ))
                .unwrap()
                .as_bool()
                .unwrap()
            );
            assert!(
                !map.get(serde_yaml::Value::String(
                    "status_server_enabled".to_string()
                ))
                .unwrap()
                .as_bool()
                .unwrap()
            );
            assert_eq!(
                9090,
                map.get(serde_yaml::Value::String("status_server_port".to_string()))
                    .unwrap()
                    .as_i64()
                    .unwrap()
            );
            assert_eq!(
                "new_key",
                map.get(serde_yaml::Value::String("license_key".to_string()))
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
            assert!(
                map.get(serde_yaml::Value::String(
                    "is_integrations_only".to_string()
                ))
                .is_none()
            );
            assert!(
                map.get(serde_yaml::Value::String("extra_field".to_string()))
                    .is_some()
            );
        } else {
            panic!("Expected a YAML mapping");
        }
    }
}
