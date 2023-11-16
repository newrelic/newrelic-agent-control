use crate::context::Context;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};
use crate::super_agent::super_agent::SuperAgentEvent;

pub struct SubAgentRemoteConfigPublisher {
    ctx: Context<Option<SuperAgentEvent>>,
}

impl SubAgentRemoteConfigPublisher {
    pub fn new(ctx: Context<Option<SuperAgentEvent>>) -> Self {
        SubAgentRemoteConfigPublisher { ctx }
    }
}

impl RemoteConfigPublisher for SubAgentRemoteConfigPublisher {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> SuperAgentEvent {
        SuperAgentEvent::SubAgentRemoteConfigValid(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> SuperAgentEvent {
        SuperAgentEvent::SubAgentRemoteConfigInvalid(err)
    }

    fn publish_event(&self, event: SuperAgentEvent) -> Result<(), RemoteConfigPublisherError> {
        return self
            .ctx
            .cancel_all(Some(event))
            .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
