use super::RemoteConfig;
use crate::agent_control::config::{AgentControlDynamicConfig, AgentTypeFQN};
use crate::agent_type::agent_type_registry::AgentRepositoryError;
use std::fmt::Display;
use thiserror::Error;

pub mod dynamic_config;
pub mod regexes;
pub mod signature;

/// Represents a validator for config remote
pub trait RemoteConfigValidator {
    type Err: Display;
    fn validate(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        remote_config: &RemoteConfig,
    ) -> Result<(), Self::Err>;
}

#[derive(Error, Debug)]
pub enum DynamicConfigValidatorError {
    #[error("validating dynamic config`{0}`")]
    AgentRepositoryError(#[from] AgentRepositoryError),
}

/// Represents a validator for dynamic config
pub trait DynamicConfigValidator {
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub RemoteConfigValidatorMock {}

        impl RemoteConfigValidator for RemoteConfigValidatorMock {
            type Err = String;

            fn validate(
                &self,
                agent_type_fqn: &AgentTypeFQN,
                remote_config: &RemoteConfig,
            ) -> Result<(), <Self as RemoteConfigValidator>::Err>;
        }
    }

    mock! {
        pub DynamicConfigValidatorMock {}

        impl DynamicConfigValidator for DynamicConfigValidatorMock {
            fn validate(
                &self,
                dynamic_config: &AgentControlDynamicConfig,
            ) -> Result<(), DynamicConfigValidatorError>;
        }
    }
}
