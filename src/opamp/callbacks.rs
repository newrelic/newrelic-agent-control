use crate::config::super_agent_configs::AgentID;
use crate::opamp::remote_config::RemoteConfigError;
use crate::opamp::remote_config_updater::RemoteConfigUpdater;
use log::trace;
use opamp_client::{
    error::ConnectionError,
    http::HttpClientError,
    opamp::proto::{
        EffectiveConfig, OpAmpConnectionSettings, ServerErrorResponse, ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use std::str;
use std::str::Utf8Error;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
pub enum AgentCallbacksError {
    #[error("deserialization error: `{0}`")]
    DeserializationError(#[from] RemoteConfigError),

    #[error("Invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),

    #[error("agent remote config with empty config body")]
    EmptyRemoteConfig,

    #[error("unable to send event through context")]
    ContextError,
}

pub struct AgentCallbacks<T>
where
    T: RemoteConfigUpdater,
{
    agent_id: AgentID,
    remote_config_updater: T,
}

impl<T> AgentCallbacks<T>
where
    T: RemoteConfigUpdater,
{
    pub fn new(agent_id: AgentID, remote_config_updater: T) -> Self {
        Self {
            agent_id,
            remote_config_updater,
        }
    }
}

impl<T> Callbacks for AgentCallbacks<T>
where
    T: RemoteConfigUpdater,
{
    type Error = AgentCallbacksError;

    fn on_connect(&self) {}

    fn on_connect_failed(&self, err: ConnectionError) {
        log_on_http_status_code(&err);
    }

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_message(&self, msg: MessageData) {
        if let Some(msg_remote_config) = msg.remote_config {
            trace!("OpAMP message received");
            let result = self
                .remote_config_updater
                .update(self.agent_id.clone(), &msg_remote_config)
                .map_err(|error| error!("{}", error));
            match result {
                Ok(()) => {
                    trace!("on message ok {:?}", msg_remote_config.clone());
                }
                Err(e) => {
                    error!("on message error {:?}", e)
                }
            }
        } else {
            trace!("Empty OpAMP message received");
        }
    }

    fn on_opamp_connection_settings(
        &self,
        _settings: &OpAmpConnectionSettings,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_opamp_connection_settings_accepted(&self, _settings: &OpAmpConnectionSettings) {}

    fn on_command(&self, _command: &ServerToAgentCommand) -> Result<(), Self::Error> {
        Ok(())
    }

    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error> {
        Ok(EffectiveConfig::default())
    }
}

fn log_on_http_status_code(err: &ConnectionError) {
    // Check if the error comes from receiving an undesired HTTP status code
    if let ConnectionError::HTTPClientError(HttpClientError::UnsuccessfulResponse(code, reason)) =
        &err
    {
        const STATUS_CODE_MSG: &str = "Received HTTP status code";
        match code {
            400 => error!("{STATUS_CODE_MSG} {code} ({reason}). The request was malformed. Possible reason: invalid ULID."),
            401 => error!("{STATUS_CODE_MSG} {code} ({reason}). Check for missing or invalid license key."
            ),
            403 => error!("{STATUS_CODE_MSG} {code} ({reason}). The account provided is not allowed to use this resource."),
            404 => error!("{STATUS_CODE_MSG} {code} ({reason}). The requested resource was not found."),
            415 => error!("{STATUS_CODE_MSG} {code} ({reason}). Content-Type or Content-Encoding for the HTTP request was wrong."),
            500 => error!("{STATUS_CODE_MSG} {code} ({reason}). Server-side problem."),
            _ => error!("{STATUS_CODE_MSG} {code} ({reason}). Reasons unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::opamp::remote_config_updater::RemoteConfigUpdaterError;
    use crate::super_agent::super_agent::SuperAgentEvent;
    use mockall::{mock, predicate};
    use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentRemoteConfig};
    use std::collections::HashMap;

    mock! {
        pub RemoteConfigUpdaterMock {}

        impl RemoteConfigUpdater for RemoteConfigUpdaterMock {
            fn on_config_ok(&self, remote_config: RemoteConfig) -> SuperAgentEvent;
            fn on_config_err(&self, err: RemoteConfigError) -> SuperAgentEvent;
            fn publish_event(&self, event: SuperAgentEvent) -> Result<(), RemoteConfigUpdaterError>;
            }
    }

    impl MockRemoteConfigUpdaterMock {
        pub fn should_on_config_ok(&mut self, remote_config: RemoteConfig, event: SuperAgentEvent) {
            let event = event.clone();
            self.expect_on_config_ok()
                .once()
                .with(predicate::eq(remote_config.clone()))
                .returning(move |_| event.clone());
        }

        pub fn should_on_config_err(&mut self, err: RemoteConfigError, event: SuperAgentEvent) {
            let event = event.clone();
            self.expect_on_config_err()
                .once()
                .with(predicate::eq(err.clone()))
                .returning(move |_| event.clone());
        }
        pub fn should_publish_event(&mut self, event: SuperAgentEvent) {
            self.expect_publish_event()
                .once()
                .with(predicate::eq(event.clone()))
                .returning(|_| Ok(()));
        }
    }

    #[test]
    fn on_message_send_correct_config() {
        let agent_id = AgentID::new("an-agent-id").unwrap();

        let msg = MessageData {
            remote_config: Option::from(AgentRemoteConfig {
                config: Option::from(AgentConfigMap {
                    config_map: HashMap::from([(
                        "my-config".to_string(),
                        AgentConfigFile {
                            body: "enable_proces_metrics: true".as_bytes().to_vec(),
                            content_type: "".to_string(),
                        },
                    )]),
                }),
                config_hash: "cool-hash".as_bytes().to_vec(),
            }),
            own_metrics: None,
            own_traces: None,
            own_logs: None,
            other_connection_settings: None,
            agent_identification: None,
        };

        let mut config_updater = MockRemoteConfigUpdaterMock::new();
        let expected_config_map = ConfigMap::new(HashMap::from([(
            "my-config".to_string(),
            "enable_proces_metrics: true".to_string(),
        )]));
        let expected_config = RemoteConfig {
            agent_id: agent_id.clone(),
            hash: Hash::new("cool-hash".to_string()),
            config_map: expected_config_map,
        };

        let expected_event = SuperAgentEvent::SuperAgentRemoteConfigValid(expected_config.clone());
        config_updater.should_on_config_ok(expected_config.clone(), expected_event.clone());

        config_updater.should_publish_event(expected_event);

        let callbacks = AgentCallbacks::new(agent_id.clone(), config_updater);

        callbacks.on_message(msg);
    }
}
