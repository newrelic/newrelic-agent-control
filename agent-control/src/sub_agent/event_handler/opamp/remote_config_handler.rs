use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::YAMLConfig;
use crate::values::yaml_config_repository::YAMLConfigRepository;
use opamp_client::StartedClient;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error};

type Status = String;
type Hash = String;
type ErrorMessage = String;

#[derive(Debug, Error)]
pub enum RemoteConfigHandlerError {
    #[error("validating remote config: `{0}`")]
    ConfigValidating(ErrorMessage),
    #[error("reporting status {0} for hash {1}: {2}")]
    StatusReporting(Status, Hash, ErrorMessage),
    #[error("storing hash and values: `{0}`")]
    HashAndValuesStore(ErrorMessage),
}

pub trait RemoteConfigHandler {
    fn handle<C>(
        &self,
        opamp_client: &C,
        agent_identity: AgentIdentity,
        config: &mut RemoteConfig,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        C: StartedClient + Send + Sync + 'static;
}

pub struct AgentRemoteConfigHandler<R, Y, S> {
    signature_validator: Arc<S>,
    sub_agent_remote_config_hash_repository: Arc<R>,
    remote_values_repo: Arc<Y>,
}

impl<R, Y, S> AgentRemoteConfigHandler<R, Y, S>
where
    R: HashRepository,
    Y: YAMLConfigRepository,
    S: RemoteConfigValidator,
{
    pub fn new(
        sub_agent_remote_config_hash_repository: Arc<R>,
        remote_values_repo: Arc<Y>,
        signature_validator: Arc<S>,
    ) -> Self {
        AgentRemoteConfigHandler {
            sub_agent_remote_config_hash_repository,
            remote_values_repo,
            signature_validator,
        }
    }

    fn report_error<C>(
        opamp_client: &C,
        config: &RemoteConfig,
        error_string: String,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        C: StartedClient + Send + Sync + 'static,
    {
        OpampRemoteConfigStatus::Error(error_string)
            .report(opamp_client, &config.hash)
            .map_err(|e| {
                RemoteConfigHandlerError::StatusReporting(
                    Status::from("error"),
                    Hash::from(&config.hash.get()),
                    e.to_string(),
                )
            })?;
        Ok(())
    }

    pub fn store_remote_config_hash_and_values(
        &self,
        remote_config: &mut RemoteConfig,
    ) -> Result<(), SubAgentError> {
        // Save the configuration hash
        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;
        // The remote configuration can be invalid (checked while deserializing)
        if let Some(err) = remote_config.hash.error_message() {
            return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
        }
        // Save the configuration values
        match process_remote_config(remote_config) {
            Err(err) => {
                // Store the hash failure if values cannot be obtained from remote config
                remote_config.hash.fail(err.to_string());
                self.sub_agent_remote_config_hash_repository
                    .save(&remote_config.agent_id, &remote_config.hash)?;
                Err(err)
            }
            // Remove previously persisted values when the configuration is empty
            Ok(None) => Ok(self
                .remote_values_repo
                .delete_remote(&remote_config.agent_id)?),
            Ok(Some(agent_values)) => Ok(self
                .remote_values_repo
                .store_remote(&remote_config.agent_id, &agent_values)?),
        }
    }
}

impl<R, Y, S> RemoteConfigHandler for AgentRemoteConfigHandler<R, Y, S>
where
    R: HashRepository,
    Y: YAMLConfigRepository,
    S: RemoteConfigValidator,
{
    /// remote_config_handler handles the remote config received by the omamp client
    /// It will
    /// * validate and persist the configuration
    /// * communicate to FM config status (applying first, applied if correct, error if failed)
    fn handle<C>(
        &self,
        opamp_client: &C,
        agent_identity: AgentIdentity,
        config: &mut RemoteConfig,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        C: StartedClient + Send + Sync + 'static,
    {
        debug!(
            agent_id = agent_identity.id.to_string(),
            select_arm = "sub_agent_opamp_consumer",
            "remote config received"
        );

        if let Err(e) = self
            .signature_validator
            .validate(&agent_identity.fqn, config)
        {
            error!(error = %e, agent_id = %agent_identity.id, hash = &config.hash.get(), "error validating signature of remote config");
            Self::report_error(opamp_client, config, e.to_string())?;
            return Err(RemoteConfigHandlerError::ConfigValidating(e.to_string()));
        }

        OpampRemoteConfigStatus::Applying
            .report(opamp_client, &config.hash)
            .map_err(|e| {
                RemoteConfigHandlerError::StatusReporting(
                    Status::from("applying"),
                    Hash::from(&config.hash.get()),
                    e.to_string(),
                )
            })?;

        if let Err(e) = self.store_remote_config_hash_and_values(config) {
            // log the error as it might be that we return a different error
            error!(error = %e, agent_id = %agent_identity.id, hash = &config.hash.get(), "error storing remote config");
            Self::report_error(opamp_client, config, e.to_string())?;
            return Err(RemoteConfigHandlerError::HashAndValuesStore(e.to_string()));
        }
        Ok(())
    }
}

fn process_remote_config(
    remote_config: &RemoteConfig,
) -> Result<Option<YAMLConfig>, SubAgentError> {
    let remote_config_value = remote_config.get_unique()?;

    if remote_config_value.is_empty() {
        return Ok(None);
    }

    Ok(Some(YAMLConfig::try_from(remote_config_value.to_string())?))
}

#[cfg(test)]
pub mod tests {
    use super::{RemoteConfigHandler, RemoteConfigHandlerError};
    use crate::opamp::remote_config::RemoteConfig;
    use crate::sub_agent::identity::AgentIdentity;
    use mockall::mock;
    use opamp_client::StartedClient;
    use predicates::prelude::predicate;

    mock! {
        pub RemoteConfigHandlerMock {}

        impl RemoteConfigHandler for RemoteConfigHandlerMock{
            fn handle<C>(
                &self,
                opamp_client: &C,
                agent_identity: AgentIdentity,
                config: &mut RemoteConfig
            ) -> Result<(), RemoteConfigHandlerError>
            where
                C: StartedClient + Send + Sync + 'static;
        }
    }

    impl MockRemoteConfigHandlerMock {
        pub fn should_handle<C>(&mut self, agent_identity: AgentIdentity, config: RemoteConfig)
        where
            C: StartedClient + Send + Sync + 'static,
        {
            self.expect_handle()
                .once()
                .with(
                    predicate::always(), // we cannot eq opamo client
                    predicate::eq(agent_identity),
                    predicate::eq(config),
                )
                .return_once(|_: &C, _, _| Ok(()));
        }
    }
}
