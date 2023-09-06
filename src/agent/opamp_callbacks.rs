use opamp_client::opamp::proto::{EffectiveConfig, OpAmpConnectionSettings, ServerErrorResponse, ServerToAgentCommand};
use opamp_client::operation::callbacks::{Callbacks, MessageData};
use thiserror::Error;

pub struct OpampCallbacks {}

impl OpampCallbacks {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Error, Debug)]
#[error("callback error mock")]
pub struct CallbacksError;

impl Callbacks for OpampCallbacks {
    type Error = CallbacksError;

    fn on_connect(&self) {
        todo!()
    }

    fn on_connect_failed(&self, _err: Self::Error) {
        todo!()
    }

    fn on_error(&self, _err: ServerErrorResponse) {
        todo!()
    }

    fn on_message(&self, _msg: MessageData) {
        todo!()
    }

    fn on_opamp_connection_settings(&self, _settings: &OpAmpConnectionSettings) -> Result<(), Self::Error> {
        todo!()
    }

    fn on_opamp_connection_settings_accepted(&self, _settings: &OpAmpConnectionSettings) {
        todo!()
    }

    fn on_command(&self, _: &ServerToAgentCommand) -> Result<(), Self::Error> {
        todo!()
    }

    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error> {
        todo!()
    }
}
