pub mod regexes;
pub mod signature;

use super::RemoteConfig;
use crate::agent_type::agent_type_id::AgentTypeID;
use regexes::RegexValidator;
use signature::validator::SignatureValidator;
use std::fmt::Display;
use thiserror::Error;

/// Represents a validator for config remote
pub trait RemoteConfigValidator {
    type Err: Display;

    fn validate(
        &self,
        agent_type_id: &AgentTypeID,
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
        agent_type_id: &AgentTypeID,
        remote_config: &RemoteConfig,
    ) -> Result<(), SupportedRemoteConfigValidatorError> {
        match self {
            Self::Signature(s) => s
                .validate(agent_type_id, remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
            Self::Regex(r) => r
                .validate(agent_type_id, remote_config)
                .map_err(|e| SupportedRemoteConfigValidatorError(e.to_string())),
        }
    }
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
                agent_type_id: &AgentTypeID,
                remote_config: &RemoteConfig,
            ) -> Result<(), <Self as RemoteConfigValidator>::Err>;
        }
    }
}
