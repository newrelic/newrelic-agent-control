//! Kubernetes-specific dynamic config validation (release-name vs. agent-id collisions).

use crate::agent_control::config::AgentControlDynamicConfig;

use super::{DynamicConfigValidator, DynamicConfigValidatorError};

/// Validates that the Kubernetes release names for AC and AC CD do not collide with agent IDs
/// to avoid modification of agent CRs during AC self-update
pub struct K8sReleaseNamesConfigValidator<V: DynamicConfigValidator> {
    inner: V,
    ac_release_name: Option<String>,
    cd_release_name: Option<String>,
}

impl<V: DynamicConfigValidator> DynamicConfigValidator for K8sReleaseNamesConfigValidator<V> {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError> {
        self.inner.validate(dynamic_config)?;

        dynamic_config.agents.keys().try_for_each(|agent_id| {
            if self
                .ac_release_name
                .clone()
                .is_some_and(|name| agent_id.as_str() == name)
            {
                return Err(validation_error(
                    agent_id.as_str(),
                    "Agent Control itself (agent-control-deployment)",
                ));
            }
            if self
                .cd_release_name
                .clone()
                .is_some_and(|name| agent_id.as_str() == name)
            {
                return Err(validation_error(
                    agent_id.as_str(),
                    "Agent Control CD (agent-control-cd)",
                ));
            }
            Ok(())
        })
    }
}

impl<V: DynamicConfigValidator> K8sReleaseNamesConfigValidator<V> {
    /// Wraps an inner validator, additionally rejecting agent ids that collide with the given
    /// Agent Control / CD release names.
    pub fn new(inner: V, ac_release_name: Option<String>, cd_release_name: Option<String>) -> Self {
        Self {
            inner,
            ac_release_name,
            cd_release_name,
        }
    }
}

fn validation_error(agent_id: &str, component: &str) -> DynamicConfigValidatorError {
    DynamicConfigValidatorError(format!(
        "agent_id '{agent_id}' collides with the release name to deploy {component}"
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
            ac_release_name: Some("agent-control-deployment".to_string()),
            cd_release_name: Some("agent-control-cd".to_string()),
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
            ac_release_name: Some("agent-control-deployment".to_string()),
            cd_release_name: Some("agent-control-cd".to_string()),
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
            ac_release_name: Some("nrdot".to_string()),
            cd_release_name: Some("agent-control-cd".to_string()),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert_matches!(result, Err(DynamicConfigValidatorError(s)) => {
            assert!(s.contains("nrdot") && s.contains("agent-control-deployment"));
        });
    }

    #[test]
    fn test_validate_cd_release_name_collision() {
        let mut inner = MockDynamicConfigValidator::new();
        inner.expect_validate().once().returning(|_| Ok(()));

        let validator = K8sReleaseNamesConfigValidator {
            inner,
            ac_release_name: Some("random-release-name".to_string()),
            cd_release_name: Some("infra-agent".to_string()),
        };

        let dynamic_config = AgentControlDynamicConfig {
            agents: helper_get_agent_list(), // contains 'infra-agent' and 'nrdot' agent-ids
            ..Default::default()
        };

        let result = validator.validate(&dynamic_config);
        assert_matches!(result, Err(DynamicConfigValidatorError(s)) => {
            assert!(s.contains("infra-agent") && s.contains("agent-control-cd"));
        });
    }
}
