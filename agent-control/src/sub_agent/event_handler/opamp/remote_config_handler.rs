use crate::agent_control::config::{AgentID, SubAgentConfig};
use crate::opamp::remote_config::hash::Hash;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::opamp::remote_config::validators::regexes::ConfigValidator;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::RemoteConfig;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, trace};

type Status = String;
type HashString = String;
type ErrorMessage = String;

#[derive(Debug, Error)]
pub enum RemoteConfigHandlerError {
    #[error("validating remote config: `{0}`")]
    ConfigValidating(ErrorMessage),
    #[error("reporting status {0} for hash {1}: {2}")]
    StatusReporting(Status, HashString, ErrorMessage),
    #[error("storing hash and values: `{0}`")]
    HashAndValuesStore(ErrorMessage),
}

pub struct RemoteConfigHandler<S, M: ConfigStatusManager> {
    // TODO: `ConfigValidator` could also implement `RemoteConfigValidator`. We may want to consider abstracting it
    // as well and implementing some sort of composite validator.
    config_validator: ConfigValidator,
    signature_validator: Arc<S>,
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    remote_config_status_manager: Arc<M>,
}

impl<S, M> RemoteConfigHandler<S, M>
where
    S: RemoteConfigValidator,
    M: ConfigStatusManager,
{
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        signature_validator: Arc<S>,
        remote_config_status_manager: Arc<M>,
    ) -> Self {
        RemoteConfigHandler {
            config_validator: ConfigValidator::default(),
            agent_id,
            agent_cfg,
            signature_validator,
            remote_config_status_manager,
        }
    }

    /// remote_config_handler handles the remote config received by the omamp client
    /// It will
    /// * validate and persist the configuration
    /// * communicate to FM config status (applying first, applied if correct, error if failed)
    pub fn handle<CB, C>(
        &self,
        opamp_client: &C,
        remote_config: RemoteConfig,
    ) -> Result<AgentRemoteConfigStatus, RemoteConfigHandlerError>
    where
        CB: Callbacks + Send + Sync + 'static,
        C: StartedClient<CB> + Send + Sync + 'static,
    {
        debug!(
            agent_id = self.agent_id.to_string(),
            select_arm = "sub_agent_opamp_consumer",
            "remote config received"
        );
        trace!(?remote_config);

        // Errors here will cause the sub-agent to continue running with the previous configuration.
        // The supervisor won't be recreated, and Fleet will send the same configuration again as the status
        // "Applied" was never reported.
        if let Err(e) = self
            .config_validator
            .validate(&self.agent_cfg.agent_type, &remote_config)
        {
            error!(error = %e, agent_id = %self.agent_id, hash = &remote_config.hash.get(), "error validating remote config with regexes");
            Self::report_error(opamp_client, &remote_config.hash, e.to_string())?;
            return Err(RemoteConfigHandlerError::ConfigValidating(e.to_string()));
        }

        if let Err(e) = self
            .signature_validator
            .validate(&self.agent_cfg.agent_type, &remote_config)
        {
            error!(error = %e, agent_id = %self.agent_id, hash = &remote_config.hash.get(), "error validating signature of remote config");
            Self::report_error(opamp_client, &remote_config.hash, e.to_string())?;
            return Err(RemoteConfigHandlerError::ConfigValidating(e.to_string()));
        }

        OpampRemoteConfigStatus::Applying
            .report(opamp_client, &remote_config.hash)
            .map_err(|e| {
                RemoteConfigHandlerError::StatusReporting(
                    Status::from("applying"),
                    remote_config.hash.get(),
                    e.to_string(),
                )
            })?;

        // Store the hash for late reporting
        let hash = remote_config.hash.clone();
        let hash_str = hash.get();

        // Convert the incoming config to our remote config status type
        let remote_config_status_result = AgentRemoteConfigStatus::try_from(remote_config)
            .and_then(|status| {
                if status.remote_config.is_some() {
                    self.remote_config_status_manager
                        .store_remote_status(&self.agent_id, &status)?;
                } else {
                    // empty remote config, delete
                    self.remote_config_status_manager
                        .delete_remote_status(&self.agent_id)?;
                }
                Ok(status)
            });

        // Did any of the operations above fail?
        remote_config_status_result.or_else(|e| {
            // log the error as it might be that we return a different error
            error!(error = %e, agent_id = %self.agent_id, hash = &hash_str, "error storing remote config");
            Self::report_error(opamp_client, &hash, e.to_string())?;
            Err(RemoteConfigHandlerError::HashAndValuesStore(e.to_string()))
         })
    }

    fn report_error<CB, C>(
        opamp_client: &C,
        hash: &Hash,
        error_string: String,
    ) -> Result<(), RemoteConfigHandlerError>
    where
        CB: Callbacks + Send + Sync + 'static,
        C: StartedClient<CB> + Send + Sync + 'static,
    {
        OpampRemoteConfigStatus::Error(error_string)
            .report(opamp_client, hash)
            .map_err(|e| {
                RemoteConfigHandlerError::StatusReporting(
                    Status::from("error"),
                    hash.get(),
                    e.to_string(),
                )
            })?;
        Ok(())
    }

    // pub fn store_remote_config_hash_and_values(
    //     &self,
    //     remote_config: RemoteConfig,
    // ) -> Result<(), SubAgentError> {
    //     // Save the configuration hash
    //     self.sub_agent_remote_config_hash_repository
    //         .save(&remote_config.agent_id, &remote_config.hash)?;
    //     // The remote configuration can be invalid (checked while deserializing)
    //     if let Some(err) = remote_config.hash.error_message() {
    //         return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
    //     }
    //     // Save the configuration values
    //     let mut remote_config = remote_config;
    //     match process_remote_config(&remote_config) {
    //         Err(err) => {
    //             // Store the hash failure if values cannot be obtained from remote config
    //             remote_config.hash.fail(err.to_string());
    //             self.sub_agent_remote_config_hash_repository
    //                 .save(&remote_config.agent_id, &remote_config.hash)?;
    //             Err(err)
    //         }
    //         // Remove previously persisted values when the configuration is empty
    //         Ok(None) => Ok(self
    //             .remote_values_repo
    //             .delete_remote(&remote_config.agent_id)?),
    //         Ok(Some(agent_values)) => Ok(self
    //             .remote_values_repo
    //             .store_remote(&remote_config.agent_id, &agent_values)?),
    //     }
    // }
}

// fn process_remote_config(
//     remote_config: &RemoteConfig,
// ) -> Result<Option<YAMLConfig>, SubAgentError> {
//     let remote_config_value = remote_config.get_unique()?;

//     if remote_config_value.is_empty() {
//         return Ok(None);
//     }

//     Ok(Some(YAMLConfig::try_from(remote_config_value.to_string())?))
// }
