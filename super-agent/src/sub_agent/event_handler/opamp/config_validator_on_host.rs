use crate::opamp::remote_config::RemoteConfig;
use crate::super_agent::config::AgentTypeFQN;
use serde_yaml::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ValidatorError {
    #[error("Invalid config: `{0}`")]
    InvalidConfig(String),
}

pub fn validate_config(
    agent_type_fqn: &AgentTypeFQN,
    remote_config: &RemoteConfig,
) -> Result<(), ValidatorError> {
    if agent_type_fqn.name() == "com.newrelic.infrastructure_agent" {
        validate_infra_agent(remote_config).unwrap();
    }
    Ok(())
}

fn validate_infra_agent(remote_config: &RemoteConfig) -> Result<(), ValidatorError> {
    // check if it has integrations
    let yaml_data = remote_config.get_unique().unwrap();

    // Deserialize the YAML string to a serde_yaml::Value
    let deserialized: Value = serde_yaml::from_str(yaml_data).unwrap();

    // Convert the Value to a HashMap<String, Value>
    let map: HashMap<String, Value> = serde_yaml::from_value(deserialized).unwrap();

    if map.contains_key("config_agent") {
        // Check if the config_agent key is present
        let config_agent_map: HashMap<String, Value> =
            serde_yaml::from_value(map.get("config_agent").unwrap().clone()).unwrap();

        // let config_agent_map: HashMap<String, Value> =
        //     serde_yaml::from_value(config_agent.clone()).unwrap();
        if config_agent_map.contains_key("strip_command_line") {
            return Err(ValidatorError::InvalidConfig(
                "strip_command_line is not allowed".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::event_handler::opamp::config_validator_on_host::validate_infra_agent;
    use crate::super_agent::config::AgentID;
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
        "#;

        let remote = RemoteConfig::new(
            AgentID::try_from(String::from("lalal")).unwrap(),
            Hash::new(String::from("hash")),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                content.to_string(),
            )]))),
        );

        validate_infra_agent(&remote);
    }
}
