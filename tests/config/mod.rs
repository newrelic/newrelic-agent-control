use std::path::Path;

use config::{Value, ValueKind};
use meta_agent::config::{
    agent_configs::MetaAgentConfig, agent_type::AgentType, resolver::Resolver,
};

fn load_config(path: &str) -> Result<MetaAgentConfig, Box<dyn std::error::Error>> {
    Ok(Resolver::retrieve_config(Some(Path::new(path)))?)
}

#[test]
fn resolve_one_agent() {
    // Build the config
    let actual = load_config("tests/config/assets/one_agent.yml").unwrap();

    let expected = MetaAgentConfig {
        agents: [(
            AgentType::InfraAgent(None),
            Value::new(None, ValueKind::Nil),
        )]
        .iter()
        .cloned()
        .collect(),
    };

    assert_eq!(actual.agents.len(), 1);
    assert_eq!(actual, expected);
}

#[test]
fn resolve_two_different_agents() {
    // Build the config
    let actual = load_config("tests/config/assets/two_agents.yml").unwrap();

    let expected = MetaAgentConfig {
        agents: [
            (
                AgentType::InfraAgent(None),
                Value::new(None, ValueKind::Nil),
            ),
            (AgentType::Nrdot(None), Value::new(None, ValueKind::Nil)),
        ]
        .iter()
        .cloned()
        .collect(),
    };

    assert_eq!(actual.agents.len(), 2);
    assert_eq!(actual, expected);
}

#[test]
fn resolve_same_type_agents() {
    // Build the config
    let actual = load_config("tests/config/assets/repeated_types.yml").unwrap();

    let expected = MetaAgentConfig {
        agents: [
            (
                AgentType::InfraAgent(None),
                Value::new(None, ValueKind::Nil),
            ),
            (
                AgentType::InfraAgent(Some("otherinstance".to_string())),
                Value::new(None, ValueKind::Nil),
            ),
            (AgentType::Nrdot(None), Value::new(None, ValueKind::Nil)),
        ]
        .iter()
        .cloned()
        .collect(),
    };

    assert_eq!(actual.agents.len(), 3);
    assert_eq!(actual, expected);
}

#[test]
fn resolve_agents_with_custom_configs() {
    // Build the config
    let actual = load_config("tests/config/assets/with_custom_configs.yml").unwrap();

    // Deserializing with the serde_yaml crate because putting
    // the literal Value representations here is too verbose!
    let expected = serde_yaml::from_str::<MetaAgentConfig>(
        r#"
agents:
  nr_infra_agent:
    configValue: value
    configList: [value1, value2]
    configMap:
      key1: value1
      key2: value2
  nr_otel_collector:
  nr_infra_agent/otherinstance:
    otherConfigValue: value
    otherConfigList: [value1, value2]
    otherConfigMap:
      key1: value1
      key2: value2

            "#,
    )
    .unwrap();

    assert_eq!(actual.agents.len(), 3);
    assert_eq!(actual, expected);
}

#[test]
fn resolve_config_with_unexpected_fields() {
    let actual = load_config("tests/config/assets/non_agent_configs.yml");
    assert!(actual.is_err());
    assert!(actual
        .unwrap_err()
        .to_string()
        .contains("unknown field `this_is_another_random_config`"));
}

#[test]
fn resolve_empty_agents_field() {
    let actual = load_config("tests/config/assets/empty_agents.yml");
    assert!(actual.is_err());
}

#[test]
fn resolve_custom_agent_with_invalid_config() {
    let actual = load_config("tests/config/assets/custom_agent_no_bin.yml");
    assert!(actual.is_err());
    assert!(actual
        .unwrap_err()
        .to_string()
        .contains("custom agent type `custom_agent/nobin` must have a `bin` key"));
}
