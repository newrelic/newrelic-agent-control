use std::collections::HashMap;
use std::str;
use opamp_client::{
    error::ConnectionError,
    http::HttpClientError,
    opamp::proto::{
        EffectiveConfig,
        OpAmpConnectionSettings,
        ServerErrorResponse, ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use thiserror::Error;
use crate::agent::AgentEvent;
use crate::config::agent_configs::AgentID;
use crate::config::remote_config::{RemoteConfig, RemoteConfigError};
use crate::context::Context;
use tracing::error;

pub struct AgentCallbacks {
    agent_id: AgentID,
    ctx: Context<Option<AgentEvent>>,
}

#[derive(Debug, Error)]
pub enum AgentCallbacksError {}

impl AgentCallbacks {
    pub fn new(ctx: Context<Option<AgentEvent>>, agent_id: AgentID) -> Self {
        Self{ ctx, agent_id }
    }
}

impl Callbacks for AgentCallbacks {
    type Error = AgentCallbacksError;

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_connect(&self) {}

    fn on_message(&self, msg: MessageData) {
        let agent_id = self.agent_id.clone();
        if let Some(msg_remote_config) = msg.remote_config {
            if let Some(msg_config_map) = msg_remote_config.config {
                let config = msg_config_map.config_map.into_iter().try_fold(
                    HashMap::new(),
                    |mut result, (key, value)| {
                        let body = match str::from_utf8(&value.body) {
                            Ok(parsed_body) => {
                                result.insert(key, parsed_body.to_string());
                                Ok(result)
                            },
                            Err(e) => Err(e),
                        };
                        body
                    },
                );

                match config {
                    Err(e) => {
                        self.ctx.cancel_all(Some(AgentEvent::RemoteConfig(Err(RemoteConfigError::UTF8(e))))).unwrap();
                    },
                    Ok(config) => {
                        let remote_config = RemoteConfig{
                            agent_id,
                            hash: str::from_utf8(&msg_remote_config.config_hash).unwrap().to_string(),
                            config_map: config,
                        };
                        self.ctx.cancel_all(Some(AgentEvent::RemoteConfig(Ok(remote_config)))).unwrap();
                    },
                }
            }
        }
    }

    fn on_command(&self, _command: &ServerToAgentCommand) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_connect_failed(&self, err: ConnectionError) {
        log_on_http_status_code(&err);
    }

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
    use std::thread::spawn;
    use log::debug;
    use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentRemoteConfig};
    use tracing::info;
    use super::*;

    #[test]
    fn on_message() {
        let ctx: Context<Option<AgentEvent>> = Context::new();
        let agent_id= AgentID::new("an-agent-id".to_string());

        spawn({
            let ctx = ctx.clone();
            move || {
                let callbacks = AgentCallbacks::new(ctx, agent_id);
                let msg = MessageData{
                    remote_config: Option::from(AgentRemoteConfig {
                        config: Option::from(AgentConfigMap {
                            config_map: HashMap::from(
                                [(
                                    "my-config".to_string(),
                                    AgentConfigFile {
                                        body: "enable_proces_metrics: true".as_bytes().to_vec(),
                                        content_type: "".to_string(),
                                    },
                                )],
                            ),
                        }),
                        config_hash: "cool-hash".as_bytes().to_vec(),
                    }),
                    own_metrics: None,
                    own_traces: None,
                    own_logs: None,
                    other_connection_settings: None,
                    agent_identification: None,
                };

                callbacks.on_message(msg);
            }
        });

        let Some(event) = ctx.wait_condvar().unwrap() else { unreachable!() };

        let AgentEvent::RemoteConfig(remote_config) = event else { unreachable!() };
        let result = remote_config.unwrap();

        assert_eq!(AgentID::new("an-agent-id".to_string()), result.agent_id);
        assert_eq!("cool-hash".to_string(), result.hash);
        assert_eq!(
            &"enable_proces_metrics: true".to_string(),
            result.config_map.get(&"my-config".to_string()).unwrap(),
        );
    }
}