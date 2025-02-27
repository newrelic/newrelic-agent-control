use super::RemoteConfig;
use crate::agent_control::config::AgentTypeFQN;
use std::fmt::Display;
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
}
