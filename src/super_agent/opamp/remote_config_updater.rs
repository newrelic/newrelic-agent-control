use crate::context::Context;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_updater::{RemoteConfigUpdater, RemoteConfigUpdaterError};
use crate::super_agent::super_agent::SuperAgentEvent;

pub struct SuperAgentRemoteConfigUpdater {
    ctx: Context<Option<SuperAgentEvent>>,
}

impl SuperAgentRemoteConfigUpdater {
    pub fn new(ctx: Context<Option<SuperAgentEvent>>) -> Self {
        SuperAgentRemoteConfigUpdater { ctx }
    }
}

impl RemoteConfigUpdater for SuperAgentRemoteConfigUpdater {
    fn on_config_ok(&self, remote_config: RemoteConfig) -> SuperAgentEvent {
        SuperAgentEvent::SuperAgentRemoteConfigValid(remote_config)
    }

    fn on_config_err(&self, err: RemoteConfigError) -> SuperAgentEvent {
        SuperAgentEvent::SuperAgentRemoteConfigInvalid(err)
    }

    fn publish_event(&self, event: SuperAgentEvent) -> Result<(), RemoteConfigUpdaterError> {
        return self
            .ctx
            .cancel_all(Some(event))
            .map_err(|_| RemoteConfigUpdaterError::PublishEventError);
    }
}
