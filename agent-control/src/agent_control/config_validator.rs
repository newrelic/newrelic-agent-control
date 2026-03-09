use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::agent_type_registry::AgentRegistry;
use std::sync::Arc;
use thiserror::Error;

pub mod k8s;

#[derive(Error, Debug)]
#[error("config validation failed: {0}")]
pub struct DynamicConfigValidatorError(String);

/// Represents a validator for dynamic config
pub trait DynamicConfigValidator {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError>;
}

/// Validator that checks the agent type exists in the agent type registry.
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
        // Validate each agent's type exists in registry
        for (agent_id, sub_agent_cfg) in &dynamic_config.agents {
            let agent_type = self
                .agent_type_registry
                .get(sub_agent_cfg.agent_type.to_string().as_str())
                .map_err(|err| {
                    DynamicConfigValidatorError(format!(
                        "AgentType registry check failed for agent '{}': {}",
                        agent_id, err
                    ))
                })?;

            // If agent declares a parent, validate parent agent exists in config
            if let Some(parent_agent_type) = agent_type.agent_type_id.parent_agent() {
                let parent_exists = dynamic_config
                    .agents
                    .values()
                    .any(|cfg| &cfg.agent_type == parent_agent_type);

                if !parent_exists {
                    return Err(DynamicConfigValidatorError(format!(
                        "Agent '{}' declares parent agent '{}' but no agent with that type exists in config",
                        agent_id,
                        parent_agent_type
                    )));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistry;
    use crate::agent_type::definition::AgentTypeDefinition;
    use mockall::mock;

    mock! {
        pub DynamicConfigValidator {}

        impl DynamicConfigValidator for DynamicConfigValidator {
            fn validate(
                &self,
                dynamic_config: &AgentControlDynamicConfig,
            ) -> Result<(), DynamicConfigValidatorError>;
        }
    }

    /// A [DynamicConfigValidator] implementation for testing purposes. It always returns Ok when valid is true
    /// and an Error when valid is false.
    pub struct TestDynamicConfigValidator {
        pub valid: bool,
    }

    impl DynamicConfigValidator for TestDynamicConfigValidator {
        fn validate(
            &self,
            _dynamic_config: &AgentControlDynamicConfig,
        ) -> Result<(), DynamicConfigValidatorError> {
            if self.valid {
                Ok(())
            } else {
                Err(DynamicConfigValidatorError("not-found".to_string()))
            }
        }
    }

    #[test]
    fn test_existing_agent_type_validation() {
        let mut registry = MockAgentRegistry::new();

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
        let mut registry = MockAgentRegistry::new();
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
