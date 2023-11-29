use crate::context::Context;
use crate::event::event::{Event, OpAMPEvent};
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SuperAgentRemoteConfigPublisher {
    ctx: Context<Option<Event>>,
}

impl SuperAgentRemoteConfigPublisher {
    pub fn new(ctx: Context<Option<Event>>) -> Self {
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
        return self
            .ctx
            .cancel_all(Some(opamp_event.into()))
            .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
