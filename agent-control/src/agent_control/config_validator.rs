//! Validation of the dynamic (remotely-mutable) part of the Agent Control configuration.

use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::registry::AgentTypeRegistry;
use std::sync::Arc;
use thiserror::Error;

pub mod k8s;

/// Error returned when a dynamic configuration fails validation.
#[derive(Error, Debug)]
#[error("config validation failed: {0}")]
pub struct DynamicConfigValidatorError(String);

/// Represents a validator for dynamic config
pub trait DynamicConfigValidator {
    /// Validates the given dynamic configuration, returning an error when it is not acceptable.
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError>;
}

/// Validator that checks the agent type exists in the agent type registry.
pub struct RegistryDynamicConfigValidator<R: AgentTypeRegistry> {
    agent_type_registry: Arc<R>,
}

impl<R: AgentTypeRegistry> RegistryDynamicConfigValidator<R> {
    /// Builds a validator backed by the given agent type registry.
    pub fn new(agent_type_registry: Arc<R>) -> Self {
        Self {
            agent_type_registry,
        }
    }
}

impl<R: AgentTypeRegistry> DynamicConfigValidator for RegistryDynamicConfigValidator<R> {
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
                    .get(&sub_agent_cfg.agent_type)
                    .map_err(|err| {
                        DynamicConfigValidatorError(format!(
                            "AgentType registry check failed: {err}"
                        ))
                    })?;
                Ok(())
            })
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
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
        let mut registry = MockAgentTypeRegistry::new();

        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());

        //Expectations
        registry.should_get(
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
            &agent_type_definition,
        );

        let dynamic_config = serde_saphyr::from_str::<AgentControlDynamicConfig>(
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
        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get_not_found(AgentTypeID::try_from("ns/another:0.0.1").unwrap());

        let dynamic_config = serde_saphyr::from_str::<AgentControlDynamicConfig>(
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
