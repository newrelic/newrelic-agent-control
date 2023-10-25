use crate::config::remote_config::{RemoteConfig, RemoteConfigError};
use crate::config::remote_config_hash::Hash;
use crate::config::super_agent_configs::AgentID;
use crate::context::Context;
use crate::super_agent::super_agent::SuperAgentEvent;
use opamp_client::opamp::proto::AgentRemoteConfig;
use opamp_client::{
    error::ConnectionError,
    http::HttpClientError,
    opamp::proto::{
        EffectiveConfig, OpAmpConnectionSettings, ServerErrorResponse, ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use std::collections::HashMap;
use std::str;
use thiserror::Error;
use tracing::error;

pub struct AgentCallbacks {
    agent_id: AgentID,
    ctx: Context<Option<SuperAgentEvent>>,
}

#[derive(Debug, Error)]
pub enum AgentCallbacksError {}

impl AgentCallbacks {
    pub fn new(ctx: Context<Option<SuperAgentEvent>>, agent_id: AgentID) -> Self {
        Self { ctx, agent_id }
    }

    fn get_remote_config(&self, agent_id: AgentID, msg_remote_config: &AgentRemoteConfig) {
        if let Some(msg_config_map) = &msg_remote_config.config {
            //Check if hash is empty
            let config = msg_config_map.config_map.iter().try_fold(
                HashMap::new(),
                |mut result, (key, value)| {
                    let body = match str::from_utf8(&value.body) {
                        Ok(parsed_body) => {
                            result.insert(key.clone(), parsed_body.to_string());
                            Ok(result)
                        }
                        Err(e) => Err(e),
                    };
                    body
                },
            );

            let current_hash = str::from_utf8(&msg_remote_config.config_hash)
                .unwrap()
                .to_string();

            match config {
                Err(e) => {
                    self.ctx
                        .cancel_all(Some(SuperAgentEvent::RemoteConfig(Err(
                            RemoteConfigError::UTF8(current_hash, e.to_string()),
                        ))))
                        .unwrap();
                }
                Ok(config) => {
                    let remote_config = RemoteConfig {
                        agent_id,
                        hash: Hash::new(current_hash),
                        config_map: config,
                    };
                    self.ctx
                        .cancel_all(Some(SuperAgentEvent::RemoteConfig(Ok(remote_config))))
                        .unwrap();
                }
            }
        }
    }
}

impl Callbacks for AgentCallbacks {
    type Error = AgentCallbacksError;

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_connect(&self) {}

    fn on_message(&self, msg: MessageData) {
        let agent_id = self.agent_id.clone();
        if let Some(msg_remote_config) = msg.remote_config {
            self.get_remote_config(agent_id, &msg_remote_config);
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
    use super::*;
    use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentRemoteConfig};
    use std::thread::spawn;

    #[test]
    fn on_message_send_correct_config() {
        let ctx: Context<Option<SuperAgentEvent>> = Context::new();
        let agent_id = AgentID::new("an-agent-id");

        spawn({
            let ctx = ctx.clone();
            move || {
                let callbacks = AgentCallbacks::new(ctx, agent_id);
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

                callbacks.on_message(msg);
            }
        });

        let Some(event) = ctx.wait_condvar().unwrap() else {
            unreachable!()
        };

        let SuperAgentEvent::RemoteConfig(remote_config) = event else {
            unreachable!()
        };
        let result = remote_config.unwrap();

        assert_eq!(AgentID::new("an-agent-id"), result.agent_id);
        assert_eq!("cool-hash".to_string(), result.hash.get());
        assert_eq!(
            &"enable_proces_metrics: true".to_string(),
            result.config_map.get(&"my-config".to_string()).unwrap(),
        );
    }
}
