use std::str;
use std::str::Utf8Error;

use thiserror::Error;
use tracing::{error, trace};

use crate::config::super_agent_configs::AgentID;
use crate::opamp::remote_config::{ConfigMap, RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::super_agent::SuperAgentEvent;
use opamp_client::opamp::proto::AgentRemoteConfig;
//TODO this callbacks thing is just is a draft idea

#[derive(Error, Debug)]
pub enum RemoteConfigPublisherError {
    #[error("unable to publish remote config event")]
    PublishEventError,
    #[error("Invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),
    // This is not actually an error, this is meant to delete configs
    #[error("empty remote config")]
    EmptyRemoteConfig,
}

pub trait RemoteConfigPublisher {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> SuperAgentEvent;
    fn on_config_err(&self, err: RemoteConfigError) -> SuperAgentEvent;

    fn publish_event(&self, event: SuperAgentEvent) -> Result<(), RemoteConfigPublisherError>;

    fn update(
        &self,
        agent_id: AgentID,
        msg_remote_config: &AgentRemoteConfig,
    ) -> Result<(), RemoteConfigPublisherError> {
        if let Some(msg_config_map) = &msg_remote_config.config {
            //Check if hash is empty
            let config: Result<ConfigMap, RemoteConfigError> = msg_config_map.try_into();

            let current_hash = str::from_utf8(&msg_remote_config.config_hash)?.to_string();
            trace!(
                "OpAMP message received with remote config hash: {}",
                current_hash
            );

            let event = match config {
                Err(e) => {
                    error!(
                        "invalid config received for agent_id: {}, hash: {}",
                        &agent_id, &current_hash
                    );
                    self.on_config_err(RemoteConfigError::InvalidConfig(
                        current_hash,
                        e.to_string(),
                    ))
                }
                Ok(config) => {
                    trace!(
                        "remote config received for agent_id: {} , hash: {} , config: {:?}",
                        &agent_id,
                        &current_hash,
                        &config
                    );

                    let remote_config = RemoteConfig {
                        agent_id,
                        hash: Hash::new(current_hash),
                        config_map: config,
                    };

                    self.on_config_ok(remote_config)
                }
            };
            return self.publish_event(event);
        }
        Err(RemoteConfigPublisherError::EmptyRemoteConfig)
    }
}
