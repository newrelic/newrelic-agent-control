pub mod regexes;
pub mod signature;
pub mod values;

use super::RemoteConfig;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
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
        agent_identity: &AgentIdentity,
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
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
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
            Self::Values(v) => v
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
