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
pub enum AgentCallbacksError {

}

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
