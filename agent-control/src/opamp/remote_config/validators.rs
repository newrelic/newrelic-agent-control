pub mod regexes;
pub mod signature;
pub mod values;

use super::RemoteConfig;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use regexes::RegexValidator;
use signature::validator::SignatureValidator;
use std::fmt::Display;
use thiserror::Error;
use values::ValuesValidator;

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
pub enum SupportedRemoteConfigValidator<A> {
    Signature(SignatureValidator),
    Regex(RegexValidator),
    Values(ValuesValidator<A>),
}

impl<A> RemoteConfigValidator for SupportedRemoteConfigValidator<A>
where
    A: EffectiveAgentsAssembler,
{
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
            Self::Values(v) => v
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
