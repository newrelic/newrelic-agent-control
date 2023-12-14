use crate::event::channel::EventPublisher;
use crate::event::event::OpAMPEvent;
use crate::event::Publisher;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SuperAgentRemoteConfigPublisher {
    ctx: EventPublisher<OpAMPEvent>,
}

impl SuperAgentRemoteConfigPublisher {
    pub fn new(ctx: EventPublisher<OpAMPEvent>) -> Self {
        SuperAgentRemoteConfigPublisher { ctx }
    }
}

impl RemoteConfigPublisher for SuperAgentRemoteConfigPublisher {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> OpAMPEvent {
        OpAMPEvent::ValidRemoteConfigReceived(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> OpAMPEvent {
        OpAMPEvent::InvalidRemoteConfigReceived(err)
    }

    fn publish_event(&self, opamp_event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError> {
        return Ok(self.ctx.publish(opamp_event));
    }
}
