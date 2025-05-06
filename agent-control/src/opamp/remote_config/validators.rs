pub mod regexes;
pub mod signature;

use super::RemoteConfig;
use crate::sub_agent::identity::AgentIdentity;
use regexes::RegexValidator;
use signature::validator::SignatureValidator;
use std::fmt::Display;
use thiserror::Error;

/// Represents a validator for config remote
pub trait RemoteConfigValidator {
    type Err: Display;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &RemoteConfig,
    ) -> Result<(), Self::Err>;
}

#[derive(Error, Debug)]
#[error("{0}")]
/// Represents an error for RemoteConfigValidatorImpl
pub struct SupportedRemoteConfigValidatorError(String);
/// Variants of Implementations of [RemoteConfigValidator] to facilitate Static Dispatch.
pub enum SupportedRemoteConfigValidator {
    Signature(SignatureValidator),
    Regex(RegexValidator),
}

impl RemoteConfigValidator for SupportedRemoteConfigValidator {
    type Err = SupportedRemoteConfigValidatorError;
    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &RemoteConfig,
    ) -> Result<(), SupportedRemoteConfigValidatorError> {
        match self {
            Self::Signature(s) => s
                .validate(agent_identity, remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
            Self::Regex(r) => r
                .validate(agent_identity, remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
        }
    }
}

#[cfg(test)]
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
                remote_config: &RemoteConfig,
            ) -> Result<(), <Self as RemoteConfigValidator>::Err>;
        }
    }

    impl MockRemoteConfigValidator {
        pub fn should_validate(
            &mut self,
            agent_identity: &AgentIdentity,
            remote_config: &RemoteConfig,
            result: Result<(), <Self as RemoteConfigValidator>::Err>,
        ) {
            self.expect_validate()
                .once()
                .with(
                    predicate::eq(agent_identity.clone()),
                    predicate::eq(remote_config.clone()),
                )
                .return_once(move |_, _| result);
        }
    }
}
