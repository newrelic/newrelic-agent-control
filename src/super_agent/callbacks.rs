use opamp_client::{
    opamp::proto::{
        EffectiveConfig, OpAmpConnectionSettings, ServerErrorResponse, ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use thiserror::Error;

pub struct AgentCallbacks;

#[derive(Debug, Error)]
pub enum AgentCallbacksError {}

impl Callbacks for AgentCallbacks {
    type Error = AgentCallbacksError;

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_connect(&self) {}

    fn on_message(&self, _msg: MessageData) {}

    fn on_command(&self, _command: &ServerToAgentCommand) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_connect_failed(&self, _err: Self::Error) {}

    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error> {
        Ok(EffectiveConfig::default())
    }

    fn on_opamp_connection_settings(
        &self,
        _settings: &OpAmpConnectionSettings,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_opamp_connection_settings_accepted(&self, _settings: &OpAmpConnectionSettings) {}
}
