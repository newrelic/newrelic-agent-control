//! Remote configuration validators (signature and regex) and their static-dispatch enum.
pub mod regexes;
pub mod signature;

use super::OpampRemoteConfig;
use crate::sub_agent::identity::AgentIdentity;
use regexes::RegexValidator;
use signature::validator::SignatureValidator;
use std::{fmt::Display, sync::Arc};
use thiserror::Error;

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

#[derive(Error, Debug)]
#[error("{0}")]
/// Represents an error for RemoteConfigValidatorImpl
pub struct SupportedRemoteConfigValidatorError(String);

/// Variants of Implementations of [RemoteConfigValidator] to facilitate Static Dispatch.
pub enum SupportedRemoteConfigValidator {
    /// Validates remote config signatures.
    Signature(Arc<SignatureValidator>),
    /// Validates remote config content against denied-pattern regexes.
    Regex(RegexValidator),
}

impl RemoteConfigValidator for SupportedRemoteConfigValidator {
    type Err = SupportedRemoteConfigValidatorError;
    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        opamp_remote_config: &OpampRemoteConfig,
    ) -> Result<(), SupportedRemoteConfigValidatorError> {
        match self {
            Self::Signature(s) => s
                .validate(agent_identity, opamp_remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
            Self::Regex(r) => r
                .validate(agent_identity, opamp_remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
        }
    }
}

#[cfg(test)]
#[allow(missing_docs)] // test-support code
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
