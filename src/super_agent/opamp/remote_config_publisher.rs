use crate::event::event::{Event, OpAMPEvent};
use crate::event::EventPublisher;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};

pub struct SuperAgentRemoteConfigPublisher<P>
where
    P: EventPublisher<Event>,
{
    ctx: P,
}

impl<P> SuperAgentRemoteConfigPublisher<P>
where
    P: EventPublisher<Event>,
{
    pub fn new(ctx: P) -> Self {
        SuperAgentRemoteConfigPublisher { ctx }
    }
}

impl<P> RemoteConfigPublisher for SuperAgentRemoteConfigPublisher<P>
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
            .ctx
            .cancel_all(Some(opamp_event.into()))
            .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
