use crate::super_agent::config::AgentTypeFQN;
use regex::Regex;
use serde::Deserialize;
use serde_yaml::Error;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use thiserror::Error;
use tracing::error;

pub const FILE_SEPARATOR: &str = ".";
// Used to replace temporarily the . separator on files to not treat them as leafs on the hashmap
pub const FILE_SEPARATOR_REPLACE: &str = "#";

pub type FilePath = PathBuf;
pub type DirPath = PathBuf;

#[derive(Error, Debug)]
pub enum MigrationConfigError {
    #[error("error parsing yaml: `{0}`")]
    SerdeYaml(#[from] Error),

    #[error("config mapping should not be empty`")]
    EmptyConfigMapping,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentTypeFieldFQN(String);

impl AgentTypeFieldFQN {
    pub fn as_string(&self) -> String {
        self.0.clone()
    }
}

impl Display for AgentTypeFieldFQN {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for AgentTypeFieldFQN {
    fn from(value: String) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl From<&String> for AgentTypeFieldFQN {
    fn from(value: &String) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl From<&str> for AgentTypeFieldFQN {
    fn from(value: &str) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl PartialEq for AgentTypeFieldFQN {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl AgentTypeFieldFQN {
    pub fn as_vec(&self) -> Vec<&str> {
        self.0.split(FILE_SEPARATOR).collect::<Vec<&str>>()
    }
}

impl Eq for AgentTypeFieldFQN {}

impl Hash for AgentTypeFieldFQN {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }

    fn hash_slice<H: Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        for piece in data {
            piece.hash(state)
        }
    }
}

pub struct FileMap {
    pub file_path: FilePath,
    pub agent_type_fqn: AgentTypeFQN,
}

pub struct DirMap {
    pub file_path: FilePath,
    pub agent_type_fqn: AgentTypeFQN,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DirInfo {
    pub path: FilePath,
    pub filename_patterns: Vec<String>,
}

impl DirInfo {
    pub fn valid_filename(&self, filename: &str) -> bool {
        for filename_pattern in &self.filename_patterns {
            let re = Regex::new(filename_pattern)
                .unwrap_or_else(|_| panic!("invalid filename_pattern: {}", filename_pattern));
            if re.is_match(filename) {
                return true;
            }
        }
        false
    }
}

pub type FilesMap = HashMap<AgentTypeFieldFQN, FilePath>;
pub type DirsMap = HashMap<AgentTypeFieldFQN, DirInfo>;

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct MigrationConfig {
    pub configs: Vec<MigrationAgentConfig>,
}

impl MigrationConfig {
    pub fn parse(config_content: &str) -> Result<Self, MigrationConfigError> {
        let mut config: MigrationConfig = serde_yaml::from_str(config_content)?;
        config.configs.sort_by_key(|c| c.agent_type_fqn.to_string());
        let last = config
            .configs
            .last()
            .ok_or(MigrationConfigError::EmptyConfigMapping)?
            .clone();
        config.configs = config
            .configs
            .iter_mut()
            .as_slice()
            .windows(2)
            .map(|c| {
                let mut current = c[0].clone();
                if c[0].agent_type_fqn.name() == c[1].agent_type_fqn.name()
                    && c[0].agent_type_fqn.namespace() == c[1].agent_type_fqn.namespace()
                {
                    current.next = Some(c[1].agent_type_fqn.clone());
                }
                current
            })
            .chain([last])
            .collect();
        Ok(config)
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct MigrationAgentConfig {
    pub agent_type_fqn: AgentTypeFQN,
    pub files_map: FilesMap,
    pub dirs_map: DirsMap,
    pub next: Option<AgentTypeFQN>,
}

impl MigrationAgentConfig {
    pub(crate) fn get_agent_type_fqn(&self) -> AgentTypeFQN {
        self.agent_type_fqn.clone()
    }
}

impl MigrationAgentConfig {
    pub fn get_file(&self, fqn_to_check: AgentTypeFieldFQN) -> Option<FilePath> {
        for (fqn, path) in self.files_map.iter() {
            if *fqn == fqn_to_check {
                return Some(path.clone());
            }
        }
        None
    }

    pub fn get_dir(&self, fqn_to_check: AgentTypeFieldFQN) -> Option<DirInfo> {
        for (fqn, dir_info) in self.dirs_map.iter() {
            if *fqn == fqn_to_check {
                return Some(dir_info.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::config_migrate::migration::config::{DirInfo, FilePath, MigrationConfig};
    use crate::config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
    use crate::super_agent::config::AgentTypeFQN;

    #[test]
    fn config_parse() {
        pub const DISORDERED_AGENT_TYPES: &str = r#"
configs:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure:0.0.2
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_ohis:
        path: /etc/newrelic-infra/integrations.d
        filename_patterns:
          - ".*\\.ya?ml$"
      logging:
        path: /etc/newrelic-infra/logging.d
        filename_patterns:
          - ".*\\.ya?ml$"
  -
    agent_type_fqn: newrelic/com.newrelic.another:1.0.0
    files_map:
      config_another: /etc/another.yml
    dirs_map:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure:1.0.1
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations:
        path: /etc/newrelic-infra/integrations.d
        filename_patterns:
          - ".*\\.ya?ml$"

      config_logging:
        path: /etc/newrelic-infra/logging.d
        filename_patterns:
          - ".*\\.ya?ml$"

  -
    agent_type_fqn: francisco-partners/com.newrelic.another:0.0.2
    files_map:
      config_another: /etc/another.yml
    dirs_map:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure:0.1.2
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations:
        path: /etc/newrelic-infra/integrations.d
        filename_patterns:
          - ".*\\.ya?ml$"
        
      config_logging:
        path: /etc/newrelic-infra/logging.d
        filename_patterns:
          - ".*\\.ya?ml$"
        
  -
    agent_type_fqn: newrelic/com.newrelic.another:0.0.1
    files_map:
      config_another: /etc/another.yml
    dirs_map:
"#;

        let expected_fqns_in_order = [
            "francisco-partners/com.newrelic.another:0.0.2"
                .try_into()
                .unwrap(),
            "newrelic/com.newrelic.another:0.0.1".try_into().unwrap(),
            "newrelic/com.newrelic.another:1.0.0".try_into().unwrap(),
            "newrelic/com.newrelic.infrastructure:0.0.2"
                .try_into()
                .unwrap(),
            "newrelic/com.newrelic.infrastructure:0.1.2"
                .try_into()
                .unwrap(),
            "newrelic/com.newrelic.infrastructure:1.0.1"
                .try_into()
                .unwrap(),
        ];
        let expected_next_fqns_in_order: Vec<Option<AgentTypeFQN>> = vec![
            None,
            Some("newrelic/com.newrelic.another:1.0.0".try_into().unwrap()),
            None,
            Some(
                "newrelic/com.newrelic.infrastructure:0.1.2"
                    .try_into()
                    .unwrap(),
            ),
            Some(
                "newrelic/com.newrelic.infrastructure:1.0.1"
                    .try_into()
                    .unwrap(),
            ),
            None,
        ];

        let config = MigrationConfig::parse(DISORDERED_AGENT_TYPES).unwrap();
        for (key, cfg) in config.configs.iter().enumerate() {
            assert_eq!(cfg.agent_type_fqn, expected_fqns_in_order[key]);
            assert_eq!(cfg.next, expected_next_fqns_in_order[key]);
        }
    }

    #[test]
    fn config_parse_error_empty_mapping() {
        pub const EMPTY_AGENT_TYPES: &str = r#"
configs: []
"#;
        assert!(MigrationConfig::parse(EMPTY_AGENT_TYPES).is_err())
    }

    #[test]
    fn test_dir_info() {
        let dir_info = DirInfo {
            filename_patterns: vec![String::from(".*\\.ya?ml$"), String::from(".*\\.otro$")],
            path: FilePath::from("some/path"),
        };

        assert!(dir_info.valid_filename("something.yaml"));
        assert!(dir_info.valid_filename("something.yml"));
        assert!(dir_info.valid_filename("something.other.yaml"));
        assert!(dir_info.valid_filename("something.otro"));
        assert!(!dir_info.valid_filename("something.yoml"));
        assert!(!dir_info.valid_filename("something.yaml.sample"));
    }

    #[test]
    fn test_dir_info_wtih_defaults() {
        let migration_config: MigrationConfig =
            MigrationConfig::parse(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING).unwrap();

        for config in migration_config.configs {
            for dir_map in config.dirs_map {
                assert!(dir_map.1.valid_filename("something.yaml"));
                assert!(dir_map.1.valid_filename("something.yml"));
                assert!(!dir_map.1.valid_filename("something.yml.sample"));
                assert!(!dir_map.1.valid_filename("something.yaml.sample"));
                assert!(!dir_map.1.valid_filename("something.yoml"));
            }
        }
    }
}
