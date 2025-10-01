use crate::agent_control::config::AgentControlDynamicConfig;

use super::{DynamicConfigValidator, DynamicConfigValidatorError};

pub struct K8sReleaseNamesConfigValidator<V: DynamicConfigValidator> {
    inner: V,
    ac_release_name: String,
    cd_release_name: String,
}

impl<V: DynamicConfigValidator> DynamicConfigValidator for K8sReleaseNamesConfigValidator<V> {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError> {
        self.inner.validate(dynamic_config)?;

        dynamic_config
            .agents
            .keys()
            .try_for_each(|agent_id| match agent_id.as_str() {
                agent_id if agent_id == self.ac_release_name => {
                    Err(validation_error(agent_id, "agent-control-deployment"))
                }
                agent_id if agent_id == self.cd_release_name => {
                    Err(validation_error(agent_id, "agent-control-cd"))
                }
                _ => Ok(()),
            })
    }
}

impl<V: DynamicConfigValidator> K8sReleaseNamesConfigValidator<V> {
    pub fn new(inner: V, ac_release_name: String, cd_release_name: String) -> Self {
        Self {
            inner,
            ac_release_name,
            cd_release_name,
        }
    }
}

fn validation_error(agent_id: &str, chart_name: &str) -> DynamicConfigValidatorError {
    DynamicConfigValidatorError(format!(
        "agent_id '{agent_id}' collides with '{chart_name}' release name"
    ))
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;
    use crate::agent_control::{
        config::tests::helper_get_agent_list, config_validator::tests::MockDynamicConfigValidator,
    };

    #[test]
    fn test_validate_no_collision() {
        let mut inner = MockDynamicConfigValidator::new();
        inner.expect_validate().once().returning(|_| Ok(()));

        let validator = K8sReleaseNamesConfigValidator {
            inner,
            ac_release_name: "agent-control-deployment".to_string(),
            cd_release_name: "agent-control-cd".to_string(),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_inner_failure() {
        let mut inner = MockDynamicConfigValidator::new();
        inner
            .expect_validate()
            .once()
            .returning(|_| Err(DynamicConfigValidatorError("inner failure".to_string())));

        let validator = K8sReleaseNamesConfigValidator {
            inner,
            ac_release_name: "agent-control-deployment".to_string(),
            cd_release_name: "agent-control-cd".to_string(),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert_matches!(result, Err(DynamicConfigValidatorError(s)) => {
            assert!(s.contains("inner failure"));
        });
    }

    #[test]
    fn test_validate_ac_release_name_collision() {
        let mut inner = MockDynamicConfigValidator::new();
        inner.expect_validate().once().returning(|_| Ok(()));

        let validator = K8sReleaseNamesConfigValidator {
            inner,
            ac_release_name: "nrdot".to_string(),
            cd_release_name: "agent-control-cd".to_string(),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert_matches!(result, Err(DynamicConfigValidatorError(s)) => {
            assert!(s.contains("agent-control-deployment"));
        });
    }

    #[test]
    fn test_validate_cd_release_name_collision() {
        let mut inner = MockDynamicConfigValidator::new();
        inner.expect_validate().once().returning(|_| Ok(()));

        let validator = K8sReleaseNamesConfigValidator {
            inner,
            ac_release_name: "random-release-name".to_string(),
            cd_release_name: "infra-agent".to_string(),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert_matches!(result, Err(DynamicConfigValidatorError(s)) => {
            assert!(s.contains("agent-control-cd"));
        });
    }
}
