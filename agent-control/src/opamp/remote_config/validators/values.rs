use crate::agent_control::agent_id::AgentID;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::environment::Environment;
use crate::opamp::remote_config::RemoteConfig;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::YAMLConfigError;
use std::sync::Arc;
use thiserror::Error;

use super::RemoteConfigValidator;

#[derive(Error, Debug)]
pub enum ValuesValidatorError {
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
    #[error("Validating config: {0}")]
    Validating(String),
}
/// Validates that a [RemoteConfig] can be rendered for a given [AgentTypeID]. Missing a
/// required variable would be some of the performed validations done.
///
/// TODO: The sub-agent currently builds the effective configuration twice: once in this validator
/// and again during the assembly process. Ideally, we should consolidate these steps to report
/// failures directly during the agent and supervisor assembly process, eliminating the need for
/// this validator. This approach was chosen as a trade-off for now.
pub struct ValuesValidator<A> {
    effective_agent_assembler: Arc<A>,
    environment: Environment,
}
impl<A> ValuesValidator<A>
where
    A: EffectiveAgentsAssembler,
{
    /// Creates a new instance of [ValuesValidator]
    pub fn new(effective_agent_assembler: Arc<A>, environment: Environment) -> Self {
        Self {
            effective_agent_assembler,
            environment,
        }
    }
}
impl<A> RemoteConfigValidator for ValuesValidator<A>
where
    A: EffectiveAgentsAssembler,
{
    type Err = ValuesValidatorError;
    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &RemoteConfig,
    ) -> Result<(), ValuesValidatorError> {
        let unique_config = remote_config
            .get_unique()
            .map_err(|e| ValuesValidatorError::Validating(e.to_string()))?;
        let config_values = unique_config
            .try_into()
            .map_err(|e: YAMLConfigError| ValuesValidatorError::Validating(e.to_string()))?;

        self.effective_agent_assembler
            .assemble_agent_from_values(config_values, agent_identity, &self.environment)
            .map_err(|err| ValuesValidatorError::InvalidConfig(err.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ValuesValidator;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::validators::values::ValuesValidatorError;
    use crate::opamp::remote_config::validators::RemoteConfigValidator;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::identity::tests::test_agent_identity;
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    #[test]
    fn test_valid_config() {
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::default();
        effective_agent_assembler
            .expect_assemble_agent_from_values()
            .once()
            .returning(|_, _, _| {
                Ok(EffectiveAgent::new(
                    test_agent_identity(),
                    Runtime {
                        deployment: Deployment {
                            on_host: None,
                            k8s: None,
                        },
                    },
                ))
            });
        ValuesValidator::new(effective_agent_assembler.into(), Environment::K8s)
            .validate(&test_agent_identity(), &fake_remote_config())
            .unwrap()
    }
    #[test]
    fn test_invalid_config() {
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::default();
        effective_agent_assembler
            .expect_assemble_agent_from_values()
            .once()
            .returning(|_, _, _| {
                Err(crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError("test".into()))
            });
        let err = ValuesValidator::new(effective_agent_assembler.into(), Environment::K8s)
            .validate(&test_agent_identity(), &fake_remote_config())
            .unwrap_err();
        assert_matches!(err, ValuesValidatorError::InvalidConfig(_));
    }
    #[test]
    fn test_validating_errors() {
        let effective_agent_assembler = MockEffectiveAgentAssemblerMock::default();
        let err = ValuesValidator::new(effective_agent_assembler.into(), Environment::K8s)
            .validate(
                &test_agent_identity(),
                &RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
                    None,
                ),
            )
            .unwrap_err();
        assert_matches!(err, ValuesValidatorError::Validating(_));
        let effective_agent_assembler = MockEffectiveAgentAssemblerMock::default();
        let err = ValuesValidator::new(effective_agent_assembler.into(), Environment::K8s)
            .validate(
                &test_agent_identity(),
                &RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
                    Some(ConfigurationMap::new(HashMap::from([(
                        "cfg".to_string(),
                        "invalidValue".to_string(),
                    )]))),
                ),
            )
            .unwrap_err();
        assert_matches!(err, ValuesValidatorError::Validating(_));
    }

    // HELPERS

    fn fake_remote_config() -> RemoteConfig {
        RemoteConfig::new(
            AgentID::new("test").unwrap(),
            Hash::new("test_payload".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "cfg:".to_string(),
                "key: val".to_string(),
            )]))),
        )
    }
}
