use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::remote_config::{ConfigMap, RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use opamp_client::opamp::proto::AgentRemoteConfig;
use std::str;
use std::str::Utf8Error;
use thiserror::Error;
use tracing::{error, trace};
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
    fn on_config_ok(&self, remote_config: RemoteConfig) -> OpAMPEvent;
    fn on_config_err(&self, err: RemoteConfigError) -> OpAMPEvent;
    fn publish_event(&self, event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError>;

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

pub struct OpAMPRemoteConfigPublisher {
    publisher: EventPublisher<OpAMPEvent>,
}

impl OpAMPRemoteConfigPublisher {
    pub fn new(publisher: EventPublisher<OpAMPEvent>) -> Self {
        Self { publisher }
    }
}

impl RemoteConfigPublisher for OpAMPRemoteConfigPublisher {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> OpAMPEvent {
        OpAMPEvent::ValidRemoteConfigReceived(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> OpAMPEvent {
        OpAMPEvent::InvalidRemoteConfigReceived(err)
    }

    fn publish_event(&self, opamp_event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError> {
        self.publisher
            .publish(opamp_event)
            .map_err(|_| RemoteConfigPublisherError::PublishEventError)
    }
}
