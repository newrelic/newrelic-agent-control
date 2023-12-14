use crate::event::event::{Event, OpAMPEvent};
use crate::event::EventPublisher;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SuperAgentRemoteConfigPublisher {
    ctx: Box<dyn EventPublisher<Event>>,
}

impl SuperAgentRemoteConfigPublisher {
    pub fn new(ctx: impl EventPublisher<Event> + 'static) -> Self {
        SuperAgentRemoteConfigPublisher { ctx: Box::new(ctx) }
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
        return Ok(self.ctx.publish(opamp_event.into()));
        // .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
