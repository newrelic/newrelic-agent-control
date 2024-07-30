use crate::opamp::remote_config::RemoteConfig;
use crate::sub_agent::validation_regexes::{REGEX_COMMAND_FIELD, REGEX_EXEC_FIELD};
use crate::super_agent::config::AgentTypeFQN;
use regex::Regex;
use std::collections::HashMap;
use thiserror::Error;

pub const FQN_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure_agent";

#[derive(Error, Debug)]
pub enum ValidatorError {
    #[error("Invalid config: restricted values detected")]
    InvalidConfig,

    #[error("error compiling regex: `{0}`")]
    RegexError(#[from] regex::Error),
}

#[derive(Debug, PartialEq, Hash, Eq)]
struct AgentTypeFQNName(String);

/// The Config validator is responsible for matching a series of regexes on the content
/// of the retrieved remote config and returning an error if a match is found.
/// If getting the unique remote config fails, the validator will return as valid
/// because we leave that kind of error handling to the store_remote_config_hash_and_values
/// on the event_processor.
pub struct ConfigValidator {
    rules: HashMap<AgentTypeFQNName, Vec<Regex>>,
}

impl ConfigValidator {
    pub fn try_new() -> Result<Self, ValidatorError> {
        Ok(Self {
            rules: HashMap::from([(
                AgentTypeFQNName(FQN_NAME_INFRA_AGENT.to_string()),
                vec![
                    Regex::new(REGEX_COMMAND_FIELD)?,
                    Regex::new(REGEX_EXEC_FIELD)?,
                ],
            )]),
        })
    }
    pub fn validate(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        remote_config: &RemoteConfig,
    ) -> Result<(), ValidatorError> {
        let agent_type_fqn_name = AgentTypeFQNName(agent_type_fqn.name());
        if !self.rules.contains_key(&agent_type_fqn_name) {
            return Ok(());
        }

        if let Ok(raw_config) = remote_config.get_unique() {
            for regex in self.rules[&agent_type_fqn_name].iter() {
                if regex.is_match(raw_config) {
                    return Err(ValidatorError::InvalidConfig);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::{ConfigValidator, FQN_NAME_INFRA_AGENT};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use std::collections::HashMap;

    #[test]
    fn test_validate() {
        let content = r#"
        health_port: 18003
        config_agent: |+
          staging: true
          enable_process_metrics: true
          status_server_enabled: true
          status_server_port: 18003
          log:
            level: info
          license_key: {{NEW_RELIC_LICENSE_KEY}}
          custom_attributes:
            nr_deployed_by: newrelic-cli

        config_integrations:
          docker-config.yml: |
            integrations:
              - name: nri-docker
                when:
                  feature: docker_enabled
                  file_exists: /var/run/docker.sock
                interval: 15s
              # This configuration is no longer included in nri-ecs images.
              # it is kept for legacy reasons, but the new one is located in https://github.com/newrelic/nri-ecs
              - name: nri-docker
                when:
                  feature: docker_enabled
                  env_exists:
                    FARGATE: "true"
                interval: 15s
                        integrations:
              - name: nri-other
                exec: /bin/crowdstrike-falcon
                interval: 15s
        "#;
        let remote_config = RemoteConfig::new(
            AgentID::new("invented").unwrap(),
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                content.to_string(),
            )]))),
        );
        let validator = ConfigValidator::try_new().unwrap();
        let agent_type_fqn =
            AgentTypeFQN::try_from(format!("newrelic/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();
        let validation_result = validator.validate(&agent_type_fqn, &remote_config);
        assert_eq!(
            validation_result.unwrap_err().to_string(),
            "Invalid config: restricted values detected"
        );
    }
}
