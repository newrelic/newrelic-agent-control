//! Remote configuration validators.
pub mod signature;

use super::OpampRemoteConfig;
use crate::sub_agent::identity::AgentIdentity;
use std::{fmt::Display, sync::Arc};

/// Represents a validator for config remote
pub trait RemoteConfigValidator {
    /// Error type returned when validation fails.
    type Err: Display;

    /// Validates the remote config for the given agent identity.
    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &OpampRemoteConfig,
    ) -> Result<(), Self::Err>;
}

impl<T> RemoteConfigValidator for Arc<T>
where
    T: RemoteConfigValidator,
{
    type Err = T::Err;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &OpampRemoteConfig,
    ) -> Result<(), Self::Err> {
        // Double deref needed to avoid infinite recursion (`*self -> Arc<T>, **self -> T`)
        (**self).validate(agent_identity, remote_config)
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use mockall::{mock, predicate};

    mock! {
        pub RemoteConfigValidator {}

        impl RemoteConfigValidator for RemoteConfigValidator {
            type Err = String;

            fn validate(
                &self,
                agent_identity: &AgentIdentity,
                remote_config: &OpampRemoteConfig,
            ) -> Result<(), <Self as RemoteConfigValidator>::Err>;
        }
    }

    impl MockRemoteConfigValidator {
        pub fn should_validate(
            &mut self,
            agent_identity: &AgentIdentity,
            opamp_remote_config: &OpampRemoteConfig,
            result: Result<(), <Self as RemoteConfigValidator>::Err>,
        ) {
            self.expect_validate()
                .once()
                .with(
                    predicate::eq(agent_identity.clone()),
                    predicate::eq(opamp_remote_config.clone()),
                )
                .return_once(move |_, _| result);
        }
    }

    pub struct TestRemoteConfigValidator {
        pub valid: bool,
    }

    impl RemoteConfigValidator for TestRemoteConfigValidator {
        type Err = String;

        fn validate(
            &self,
            _agent_identity: &AgentIdentity,
            _remote_config: &OpampRemoteConfig,
        ) -> Result<(), Self::Err> {
            if self.valid {
                Ok(())
            } else {
                Err("invalid".to_string())
            }
        }
    }
}
