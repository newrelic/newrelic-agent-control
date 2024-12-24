use crate::agent_control::config::{AgentID, SubAgentConfig};
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::remote_config::validators::config::ConfigValidator;
use crate::opamp::remote_config::validators::signature::SignatureValidator;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::sub_agent::error::SubAgentError;
use crate::values::yaml_config::YAMLConfig;
use crate::values::yaml_config_repository::YAMLConfigRepository;
use opamp_client::operation::callbacks::Callbacks;
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

pub struct RemoteConfigHandler<R, Y> {
    config_validator: ConfigValidator,
    signature_validator: Arc<SignatureValidator>,
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    sub_agent_remote_config_hash_repository: Arc<R>,
    remote_values_repo: Arc<Y>,
}

impl<R, Y> RemoteConfigHandler<R, Y>
where
    R: HashRepository,
    Y: YAMLConfigRepository,
{
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        sub_agent_remote_config_hash_repository: Arc<R>,
        remote_values_repo: Arc<Y>,
        signature_validator: Arc<SignatureValidator>,
    ) -> Self {
        RemoteConfigHandler {
            config_validator: ConfigValidator::default(),
            agent_id,
            agent_cfg,
            sub_agent_remote_config_hash_repository,
            remote_values_repo,
            signature_validator,
        }
    }

    /// remote_config_handler handles the remote config received by the omamp client
    /// It will
    /// * validate and persist the configuration
    /// * communicate to FM config status (applying first, applied if correct, error if failed)
    pub fn handle<CB, C>(
        &self,
        opamp_client: &C,
        config: &mut RemoteConfig,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        CB: Callbacks + Send + Sync + 'static,
        C: StartedClient<CB> + Send + Sync + 'static,
    {
        debug!(
            agent_id = self.agent_id.to_string(),
            select_arm = "sub_agent_opamp_consumer",
            "remote config received"
        );

        // Errors here will cause the sub-agent to continue running with the previous configuration.
        // The supervisor won't be recreated, and Fleet will send the same configuration again as the status
        // "Applied" was never reported.
        if let Err(e) = self
            .config_validator
            .validate(&self.agent_cfg.agent_type, config)
        {
            error!(error = %e, agent_id = %self.agent_id, hash = &config.hash.get(), "error validating remote config with regexes");
            Self::report_error(opamp_client, config, e.to_string())?;
            return Err(RemoteConfigHandlerError::ConfigValidating(e.to_string()));
        }

        if let Err(e) = self
            .signature_validator
            .validate(&self.agent_cfg.agent_type, config)
        {
            error!(error = %e, agent_id = %self.agent_id, hash = &config.hash.get(), "error validating signature of remote config");
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
            error!(error = %e, agent_id = %self.agent_id, hash = &config.hash.get(), "error storing remote config");
            Self::report_error(opamp_client, config, e.to_string())?;
            return Err(RemoteConfigHandlerError::HashAndValuesStore(e.to_string()));
        }
        Ok(())
    }

    fn report_error<CB, C>(
        opamp_client: &C,
        config: &RemoteConfig,
        error_string: String,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        CB: Callbacks + Send + Sync + 'static,
        C: StartedClient<CB> + Send + Sync + 'static,
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

fn process_remote_config(
    remote_config: &RemoteConfig,
) -> Result<Option<YAMLConfig>, SubAgentError> {
    let remote_config_value = remote_config.get_unique()?;

    if remote_config_value.is_empty() {
        return Ok(None);
    }

    Ok(Some(YAMLConfig::try_from(remote_config_value.to_string())?))
}
