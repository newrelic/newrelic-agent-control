use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::agent_type_registry::AgentRegistry;
use crate::opamp::remote_config::validators::{
    DynamicConfigValidator, DynamicConfigValidatorError,
};
use std::ops::Deref;
use std::sync::Arc;

pub struct RegistryDynamicConfigValidator<R: AgentRegistry> {
    agent_type_registry: Arc<R>,
}

impl<R: AgentRegistry> RegistryDynamicConfigValidator<R> {
    pub fn new(agent_type_registry: Arc<R>) -> Self {
        Self {
            agent_type_registry,
        }
    }
}

impl<R: AgentRegistry> DynamicConfigValidator for RegistryDynamicConfigValidator<R> {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError> {
        dynamic_config
            .agents
            .values()
            .try_for_each(|sub_agent_cfg| {
                let _ = self
                    .agent_type_registry
                    .get(sub_agent_cfg.agent_type.deref())?;
                Ok(())
            })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_metadata::AgentMetadata;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::agent_type::definition::AgentTypeDefinition;
    use semver::Version;

    #[test]
    fn test_existing_agent_type_validation() {
        let mut registry = MockAgentRegistryMock::new();

        let agent_type_definition = AgentTypeDefinition::empty_with_metadata(AgentMetadata {
            name: "some_fqn".into(),
            version: Version::parse("0.0.1").unwrap(),
            namespace: "ns".into(),
        });

        //Expectations
        registry.should_get("ns/some_fqn:0.0.1".to_string(), &agent_type_definition);

        let dynamic_config = serde_yaml::from_str::<AgentControlDynamicConfig>(
            r#"
agents:
  some-agent:
    agent_type: ns/some_fqn:0.0.1
"#,
        )
        .unwrap();

        let dynamic_config_validator = RegistryDynamicConfigValidator::new(Arc::new(registry));

        assert!(dynamic_config_validator.validate(&dynamic_config).is_ok());
    }
    #[test]
    fn test_non_existing_agent_type_validation() {
        let mut registry = MockAgentRegistryMock::new();
        registry.should_not_get("ns/another:0.0.1".to_string());

        let dynamic_config = serde_yaml::from_str::<AgentControlDynamicConfig>(
            r#"
agents:
  some-agent:
    agent_type: ns/another:0.0.1
"#,
        )
        .unwrap();

        let dynamic_config_validator = RegistryDynamicConfigValidator::new(Arc::new(registry));

        assert!(dynamic_config_validator.validate(&dynamic_config).is_err());
    }
}
