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
    fn on_remote_config(&self, remote_config: RemoteConfig) -> OpAMPEvent;
    fn publish_event(&self, event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError>;

    fn update(
        &self,
        agent_id: AgentID,
        msg_remote_config: &AgentRemoteConfig,
    ) -> Result<(), RemoteConfigPublisherError> {
        let mut hash = match str::from_utf8(&msg_remote_config.config_hash) {
            Ok(hash) => Hash::new(hash.to_string()),
            Err(err) => {
                // the hash must be created to keep track of the failing remote config.
                let mut hash = Hash::new(String::new());
                hash.fail(format!("Invalid hash: {}", err));
                hash
            }
        };

        let config_map: Option<ConfigMap> = match &msg_remote_config.config {
            Some(msg_config_map) => msg_config_map
                .try_into()
                .inspect_err(|err: &RemoteConfigError| {
                    hash.fail(format!("Invalid format: {}", err))
                })
                .ok(),
            None => {
                hash.fail("Config missing".into());
                None
            }
        };

        let remote_config = RemoteConfig::new(agent_id, hash, config_map);

        trace!("remote config received: {:?}", &remote_config);

        self.publish_event(self.on_remote_config(remote_config))
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
    fn on_remote_config(&self, remote_config: RemoteConfig) -> OpAMPEvent {
        OpAMPEvent::RemoteConfigReceived(remote_config)
    }

    fn publish_event(&self, opamp_event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError> {
        self.publisher
            .publish(opamp_event)
            .map_err(|_| RemoteConfigPublisherError::PublishEventError)
    }
}
