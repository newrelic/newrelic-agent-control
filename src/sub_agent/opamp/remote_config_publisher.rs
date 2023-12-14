use crate::event::event::{Event, OpAMPEvent};
use crate::event::EventPublisher;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SubAgentRemoteConfigPublisher<P>
where
    P: EventPublisher<Event>,
{
    ctx: P,
}

impl<P> SubAgentRemoteConfigPublisher<P>
where
    P: EventPublisher<Event>,
{
    pub fn new(ctx: P) -> Self {
        SubAgentRemoteConfigPublisher { ctx }
    }
}

impl<P> RemoteConfigPublisher for SubAgentRemoteConfigPublisher<P>
where
    P: EventPublisher<Event>,
{
    fn on_config_ok(&self, remote_config: RemoteConfig) -> OpAMPEvent {
        OpAMPEvent::ValidRemoteConfigReceived(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> OpAMPEvent {
        OpAMPEvent::InvalidRemoteConfigReceived(err)
    }

    fn publish_event(&self, opamp_event: OpAMPEvent) -> Result<(), RemoteConfigPublisherError> {
        return self
            .publish_event(opamp_event.into())
            .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
