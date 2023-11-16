use crate::context::Context;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_publisher::{RemoteConfigPublisher, RemoteConfigPublisherError};
use crate::super_agent::super_agent::SuperAgentEvent;

pub struct SuperAgentRemoteConfigPublisher {
    ctx: Context<Option<SuperAgentEvent>>,
}

impl SuperAgentRemoteConfigPublisher {
    pub fn new(ctx: Context<Option<SuperAgentEvent>>) -> Self {
        SuperAgentRemoteConfigPublisher { ctx }
    }
}

impl RemoteConfigPublisher for SuperAgentRemoteConfigPublisher {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> SuperAgentEvent {
        SuperAgentEvent::SuperAgentRemoteConfigValid(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> SuperAgentEvent {
        SuperAgentEvent::SuperAgentRemoteConfigInvalid(err)
    }

    fn publish_event(&self, event: SuperAgentEvent) -> Result<(), RemoteConfigPublisherError> {
        return self
            .ctx
            .cancel_all(Some(event))
            .map_err(|_| RemoteConfigPublisherError::PublishEventError);
    }
}
