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

    fn on_connect_failed(&self, err: Self::Error) {
        todo!()
    }

    fn on_error(&self, err: ServerErrorResponse) {
        todo!()
    }

    fn on_message(&self, msg: MessageData) {
        todo!()
    }

    fn on_opamp_connection_settings(&self, settings: &OpAmpConnectionSettings) -> Result<(), Self::Error> {
        todo!()
    }

    fn on_opamp_connection_settings_accepted(&self, settings: &OpAmpConnectionSettings) {
        todo!()
    }

    fn on_command(&self, command: &ServerToAgentCommand) -> Result<(), Self::Error> {
        todo!()
    }

    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error> {
        todo!()
    }
}
