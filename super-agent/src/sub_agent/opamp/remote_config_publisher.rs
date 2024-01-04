use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SubAgentRemoteConfigPublisher {
    publisher: EventPublisher<OpAMPEvent>,
}

impl SubAgentRemoteConfigPublisher {
    pub fn new(publisher: EventPublisher<OpAMPEvent>) -> Self {
        SubAgentRemoteConfigPublisher { publisher }
    }
}

impl RemoteConfigPublisher for SubAgentRemoteConfigPublisher {
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
