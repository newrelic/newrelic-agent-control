//! Provides an abstraction over common executed methods after parsing a
//! ServerToAgent message.

use std::collections::HashMap;

use crate::{
    error::ConnectionError,
    opamp::proto::{
        AgentIdentification, AgentRemoteConfig, EffectiveConfig, OpAmpConnectionSettings,
        OtherConnectionSettings, ServerErrorResponse, ServerToAgentCommand,
        TelemetryConnectionSettings,
    },
};

/// MessageData represents a message received from the server and handled by Callbacks.
#[derive(Debug, Default, PartialEq)]
pub struct MessageData {
    /// remote_config is offered by the Server. The Agent must process it and call
    /// OpAMPClient.SetRemoteConfigStatus to indicate success or failure. If the
    /// effective config has changed as a result of processing the Agent must also call
    /// OpAMPClient.UpdateEffectiveConfig. SetRemoteConfigStatus and UpdateEffectiveConfig
    /// may be called from OnMessage handler or after OnMessage returns.
    pub remote_config: Option<AgentRemoteConfig>,

    /// Metrics connection settings offered by the Server.
    pub own_metrics: Option<TelemetryConnectionSettings>,
    /// Traces connection settings offered by the Server.
    pub own_traces: Option<TelemetryConnectionSettings>,
    /// Logging connection settings offered by the Server.
    pub own_logs: Option<TelemetryConnectionSettings>,
    /// Other connection settings offered by the Server.
    pub other_connection_settings: Option<HashMap<String, OtherConnectionSettings>>,

    /// agent_identification indicates a new identification received from the Server.
    /// The Agent must save this identification and use it in the future instantiations
    /// of OpAMPClient.
    pub agent_identification: Option<AgentIdentification>,
}

/// Callbacks is a trait for the Client to handle messages from the Server.
pub trait Callbacks {
    /// Associated type to return as Callbacks error.
    type Error: std::error::Error + Send + Sync;

    /// on_connect is called when the connection is successfully established to the Server.
    /// May be called after Start() is called and every time a connection is established to the Server.
    /// For WebSocket clients this is called after the handshake is completed without any error.
    /// For HTTP clients this is called for any request if the response status is OK.
    fn on_connect(&self);

    /// on_connect_failed is called when the connection to the Server cannot be established.
    fn on_connect_failed(&self, err: ConnectionError);

    /// on_error is called when the Server reports an error in response to some previously
    /// sent request. Useful for logging purposes. The Agent should not attempt to process
    /// the error by reconnecting or retrying previous operations. The client handles the
    /// ErrorResponse_UNAVAILABLE case internally by performing retries as necessary.
    fn on_error(&self, err: ServerErrorResponse);

    /// on_message is called when the Agent receives a message that needs processing.
    /// See MessageData definition for the data that may be available for processing.
    /// During OnMessage execution the OpAMPClient functions that change the status
    /// of the client may be called, e.g. if RemoteConfig is processed then
    /// SetRemoteConfigStatus should be called to reflect the processing result.
    /// These functions may also be called after OnMessage returns. This is advisable
    /// if processing can take a long time. In that case returning quickly is preferable
    /// to avoid blocking the OpAMPClient.
    fn on_message(&self, msg: MessageData);

    /// on_opamp_connection_settings is called when the Agent receives an OpAMP
    /// connection settings offer from the Server. Typically, the settings can specify
    /// authorization headers or TLS certificate, potentially also a different
    /// OpAMP destination to work with.
    ///
    /// The Agent should process the offer and return an error if the Agent does not
    /// want to accept the settings (e.g. if the TSL certificate in the settings
    /// cannot be verified).
    ///
    /// If on_opamp_connection_settings returns nil and then the caller will
    /// attempt to reconnect to the OpAMP Server using the new settings.
    /// If the connection fails the settings will be rejected and an error will
    /// be reported to the Server. If the connection succeeds the new settings
    /// will be used by the client from that moment on.
    ///
    /// Only one on_opamp_connection_settings call can be active at any time.
    /// See on_remote_config for the behavior.
    fn on_opamp_connection_settings(
        &self,
        settings: &OpAmpConnectionSettings,
    ) -> Result<(), Self::Error>;

    /// on_opamp_connection_settings_accepted will be called after the settings are
    /// verified and accepted (OnOpampConnectionSettingsOffer and connection using
    /// new settings succeeds). The Agent should store the settings and use them
    /// in the future. Old connection settings should be forgotten.
    fn on_opamp_connection_settings_accepted(&self, settings: &OpAmpConnectionSettings);

    /// on_command is called when the Server requests that the connected Agent perform a command.
    fn on_command(&self, command: &ServerToAgentCommand) -> Result<(), Self::Error>;

    /// get_effective_config returns the current effective config. Only one
    /// get_effective_config  call can be active at any time. Until get_effective_config
    /// returns it will not be called again.
    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error>;
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;

    use mockall::{mock, predicate};
    use thiserror::Error;

    #[derive(Error, Debug)]
    #[error("callback error mock")]
    pub(crate) struct CallbacksMockError;

    mock! {
      pub(crate) CallbacksMockall {}

      impl Callbacks for CallbacksMockall {
            type Error = CallbacksMockError;

            fn on_connect(&self);
            fn on_connect_failed(&self, err: ConnectionError);
            fn on_error(&self, err: ServerErrorResponse);
            fn on_message(&self, msg: MessageData);
            fn on_opamp_connection_settings(&self,settings: &OpAmpConnectionSettings,) -> Result<(), <Self as Callbacks>::Error>;
            fn on_opamp_connection_settings_accepted(&self, settings: &OpAmpConnectionSettings);
            fn on_command(&self, command: &ServerToAgentCommand) -> Result<(), <Self as Callbacks>::Error>;
            fn get_effective_config(&self) -> Result<EffectiveConfig, <Self as Callbacks>::Error>;
      }
    }

    impl MockCallbacksMockall {
        #[allow(dead_code)]
        pub fn should_on_connect(&mut self) {
            self.expect_on_connect().once().return_const(());
        }

        pub fn should_on_connect_failed(&mut self) {
            self.expect_on_connect_failed()
                .once()
                // .with(predicate::eq(err))
                .return_const(());
        }

        pub fn should_on_message(&mut self, data: MessageData) {
            self.expect_on_message()
                .once()
                .with(predicate::eq(data))
                .return_const(());
        }

        pub fn should_on_command(&mut self, cmd: &ServerToAgentCommand) {
            self.expect_on_command()
                .once()
                .withf({
                    let cmd = cmd.clone();
                    move |x| x == &cmd
                })
                .returning(|_| Ok(()));
        }

        pub fn should_not_on_command(&mut self) {
            self.expect_on_command().never();
        }

        #[allow(dead_code)]
        pub fn should_on_opamp_connection_settings(&mut self, ocs: &OpAmpConnectionSettings) {
            self.expect_on_opamp_connection_settings()
                .once()
                .withf({
                    let ocs = ocs.clone();
                    move |x| x == &ocs
                })
                .returning(|_| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_on_opamp_connection_settings_accepted(
            &mut self,
            ocs: &OpAmpConnectionSettings,
        ) {
            self.expect_on_opamp_connection_settings_accepted()
                .once()
                .withf({
                    let ocs = ocs.clone();
                    move |x| x == &ocs
                })
                .return_const(());
        }

        pub fn should_get_effective_config(&mut self) {
            self.expect_get_effective_config()
                .once()
                .returning(|| Ok(EffectiveConfig::default()));
        }

        pub fn should_not_get_effective_config(&mut self) {
            self.expect_get_effective_config().never();
        }
    }
}
