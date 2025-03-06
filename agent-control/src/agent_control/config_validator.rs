use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DynamicConfigValidatorError {
    #[error("`{0}`")]
    AgentRepositoryError(#[from] AgentRepositoryError),
}

/// Represents a validator for dynamic config
pub trait DynamicConfigValidator {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError>;
}

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
                    .get(sub_agent_cfg.agent_type.to_string().as_str())?;
                Ok(())
            })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::agent_type::definition::AgentTypeDefinition;
    use mockall::mock;

    mock! {
        pub DynamicConfigValidatorMock {}

        impl DynamicConfigValidator for DynamicConfigValidatorMock {
            fn validate(
                &self,
                dynamic_config: &AgentControlDynamicConfig,
            ) -> Result<(), DynamicConfigValidatorError>;
        }
    }

    #[test]
    fn test_existing_agent_type_validation() {
        let mut registry = MockAgentRegistryMock::new();

        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());

        //Expectations
        registry.should_get("ns/name:0.0.1".to_string(), &agent_type_definition);

        let dynamic_config = serde_yaml::from_str::<AgentControlDynamicConfig>(
            r#"
agents:
  some-agent:
    agent_type: ns/name:0.0.1
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
