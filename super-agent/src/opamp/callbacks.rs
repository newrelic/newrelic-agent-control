use crate::event::{
    channel::{EventPublisher, EventPublisherError},
    OpAMPEvent,
};
use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
use crate::opamp::remote_config_hash::Hash;
use crate::{opamp::remote_config::RemoteConfigError, super_agent::config::AgentID};
use opamp_client::{
    error::ConnectionError,
    error::ConnectionError::HTTPClientError,
    http::HttpClientError,
    opamp::proto::{
        AgentRemoteConfig, EffectiveConfig, OpAmpConnectionSettings, ServerErrorResponse,
        ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use std::str;
use std::str::Utf8Error;
use thiserror::Error;
use tracing::{error, trace};

#[derive(Debug, Error)]
pub enum AgentCallbacksError {
    #[error("deserialization error: `{0}`")]
    DeserializationError(#[from] RemoteConfigError),

    #[error("Invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),

    #[error("unable to publish OpAMP event")]
    PublishEventError(#[from] EventPublisherError),
}

/// This component implements the OpAMP client callbacks process the messages and publish events on `crate::event::OpAMPEvent`.
pub struct AgentCallbacks {
    agent_id: AgentID,
    publisher: EventPublisher<OpAMPEvent>,
}

impl AgentCallbacks {
    pub fn new(agent_id: AgentID, publisher: EventPublisher<OpAMPEvent>) -> Self {
        Self {
            agent_id,
            publisher,
        }
    }

    /// Assembles a `RemoteConfig` from the OpAMP message and publish the `crate::event::OpAMPEvent::RemoteConfigReceived`.
    fn process_remote_config(
        &self,
        msg_remote_config: &AgentRemoteConfig,
    ) -> Result<(), AgentCallbacksError> {
        trace!(
            agent_id = self.agent_id.to_string(),
            "OpAMP remote config message received"
        );

        let mut hash = match str::from_utf8(&msg_remote_config.config_hash) {
            Ok(hash) => Hash::new(hash.to_string()),
            Err(err) => {
                // the hash must be created to keep track of the failing remote config.
                let mut hash = Hash::new(String::new());
                hash.fail(format!("Invalid hash: {}", err));
                hash
            }
        };

        let config_map: Option<ConfigMap> = match &msg_remote_config.config {
            Some(msg_config_map) => msg_config_map
                .try_into()
                .inspect_err(|err: &RemoteConfigError| {
                    hash.fail(format!("Invalid format: {}", err))
                })
                .ok(),
            None => {
                hash.fail("Config missing".into());
                None
            }
        };

        let remote_config = RemoteConfig::new(self.agent_id.clone(), hash, config_map);

        trace!(
            agent_id = self.agent_id.to_string(),
            "remote config received: {:?}",
            &remote_config
        );

        Ok(self
            .publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))?)
    }

    fn process_on_connect_failure(&self, err: &ConnectionError) -> Result<(), AgentCallbacksError> {
        match err {
            // Check if the error comes from receiving an undesired HTTP status code
            HTTPClientError(HttpClientError::UnsuccessfulResponse(code, reason)) => {
                const STATUS_CODE_MSG: &str = "Received HTTP status code";
                match code {
                    400 => error!(agent_id = self.agent_id.to_string() , "{STATUS_CODE_MSG} {code} ({reason}). The request was malformed. Possible reason: invalid ULID"),
                    401 => error!(agent_id = self.agent_id.to_string() , "{STATUS_CODE_MSG} {code} ({reason}). Check for missing or invalid license key"
                    ),
                    403 => error!(agent_id = self.agent_id.to_string(), "{STATUS_CODE_MSG} {code} ({reason}). The account provided is not allowed to use this resource"),
                    404 => error!(agent_id = self.agent_id.to_string(), "{STATUS_CODE_MSG} {code} ({reason}). The requested resource was not found"),
                    415 => error!(agent_id = self.agent_id.to_string(), "{STATUS_CODE_MSG} {code} ({reason}). Content-Type or Content-Encoding for the HTTP request was wrong"),
                    500 => error!(agent_id = self.agent_id.to_string(), "{STATUS_CODE_MSG} {code} ({reason}). Server-side problem"),
                    _ => error!(agent_id = self.agent_id.to_string(), "{STATUS_CODE_MSG} {code} ({reason}). Reasons unknown"),
                }
                return Ok(self
                    .publisher
                    .publish(OpAMPEvent::ConnectFailed(*code, reason.clone()))?);
            }
            _ => error!(
                agent_id = self.agent_id.to_string(),
                err = err.to_string(),
                "Connecting to OpAMP server"
            ),
        }

        Ok(())
    }
}

impl Callbacks for AgentCallbacks {
    type Error = AgentCallbacksError;

    fn on_connect(&self) {
        let _ = self
            .publisher
            .publish(OpAMPEvent::Connected)
            .map_err(|error| {
                error!(
                    agent_id = self.agent_id.to_string(),
                    err = error.to_string(),
                    "processing OpAMP connect"
                )
            });
    }

    fn on_connect_failed(&self, err: ConnectionError) {
        let _ = self.process_on_connect_failure(&err).map_err(|error| {
            error!(
                agent_id = self.agent_id.to_string(),
                err = error.to_string(),
                "processing OpAMP connect fail"
            )
        });
    }

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_message(&self, msg: MessageData) {
        if let Some(msg_remote_config) = msg.remote_config {
            let _ = self
                .process_remote_config(&msg_remote_config)
                .map_err(|error| {
                    error!(
                        agent_id = self.agent_id.to_string(),
                        err = error.to_string(),
                        "processing OpAMP message"
                    )
                })
                .map(|_| {
                    trace!(
                        agent_id = self.agent_id.to_string(),
                        "on message ok {:?}",
                        msg_remote_config
                    )
                });
        } else {
            trace!(
                agent_id = self.agent_id.to_string(),
                "Empty OpAMP message received"
            );
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::event::channel::pub_sub;
    use crate::event::OpAMPEvent;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentRemoteConfig};
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_connect() {
        let (event_publisher, event_consumer) = pub_sub();

        let callbacks = AgentCallbacks::new(AgentID::new("agent").unwrap(), event_publisher);

        callbacks.on_connect();

        let event = event_consumer
            .as_ref()
            .recv_timeout(Duration::from_millis(1))
            .unwrap();

        assert_eq!(event, OpAMPEvent::Connected);
        assert!(event_consumer.as_ref().is_empty());
    }

    #[test]
    fn test_connect_fail() {
        let (event_publisher, event_consumer) = pub_sub();

        let callbacks = AgentCallbacks::new(AgentID::new("agent").unwrap(), event_publisher);

        // When a UnsuccessfulResponse error is received
        let (status, reason) = (401, "Unauthorized");
        callbacks.on_connect_failed(ConnectionError::HTTPClientError(
            HttpClientError::UnsuccessfulResponse(status, reason.to_string()),
        ));

        let event = event_consumer
            .as_ref()
            .recv_timeout(Duration::from_millis(1))
            .expect("should receive an event");

        // Then a OpAMPEvent::ConnectFailed is sent.
        assert_eq!(event, OpAMPEvent::ConnectFailed(status, reason.to_string()));
        assert!(event_consumer.as_ref().is_empty());

        // When an error without status code and reason is received
        callbacks.on_connect_failed(ConnectionError::HTTPClientError(
            HttpClientError::UreqError("Some transport error".to_string()),
        ));

        let _ = event_consumer
            .as_ref()
            .recv_timeout(Duration::from_millis(100))
            .expect_err("no event should be received");
    }

    #[test]
    fn test_remote_config() {
        let valid_hash = "hash";
        let invalid_utf = vec![128, 129];

        struct TestCase {
            name: &'static str,
            opamp_msg: Option<MessageData>, // using option here to allow taking the ownership of the MessageData which cannot be cloned.
            expected_remote_config_hash: Hash,
            expected_remote_config_config_map: Option<ConfigMap>,
        }
        impl TestCase {
            fn run(mut self) {
                let agent_id = AgentID::new("an-agent-id").unwrap();

                let (event_publisher, event_consumer) = pub_sub();

                let callbacks = AgentCallbacks::new(agent_id.clone(), event_publisher);

                callbacks.on_message(self.opamp_msg.take().unwrap());

                let event = event_consumer
                    .as_ref()
                    .recv_timeout(Duration::from_millis(1))
                    .unwrap();

                let expected_event = OpAMPEvent::RemoteConfigReceived(RemoteConfig::new(
                    agent_id.clone(),
                    self.expected_remote_config_hash.clone(),
                    self.expected_remote_config_config_map.clone(),
                ));

                assert_eq!(event, expected_event, "{}", self.name);

                assert!(event_consumer.as_ref().is_empty());
            }
        }
        let test_cases = vec![
            TestCase {
                name: "with valid values",
                opamp_msg: Some(MessageData {
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
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(ConfigMap::new(HashMap::from([(
                    "my-config".to_string(),
                    "enable_proces_metrics: true".to_string(),
                )]))),
                expected_remote_config_hash: Hash::new(valid_hash.to_string()),
            },
            TestCase {
                name: "with invalid values",
                opamp_msg: Some(MessageData {
                    remote_config: Option::from(AgentRemoteConfig {
                        config: Option::from(AgentConfigMap {
                            config_map: HashMap::from([(
                                "my-config".to_string(),
                                AgentConfigFile {
                                    body: invalid_utf.clone(),
                                    content_type: "".to_string(),
                                },
                            )]),
                        }),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: None,
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new(valid_hash.to_string());
                    expected_hash.fail(
                        "Invalid format: invalid UTF-8 sequence: `invalid utf-8 sequence of 1 bytes from index 0`".into(),
                    );
                    expected_hash
                },
            },
            TestCase {
                name: "with missing config and valid hash",
                opamp_msg: Some(MessageData {
                    remote_config: Option::from(AgentRemoteConfig {
                        config: None,
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: None,
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new(valid_hash.to_string());
                    expected_hash.fail("Config missing".into());
                    expected_hash
                },
            },
            TestCase {
                name: "with missing config and invalid hash",
                opamp_msg: Some(MessageData {
                    remote_config: Option::from(AgentRemoteConfig {
                        config: None,
                        config_hash: invalid_utf.clone(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: None,
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new("".to_string());
                    expected_hash.fail("Config missing".into());
                    expected_hash
                },
            },
            TestCase {
                name: "with valid config and invalid hash",
                opamp_msg: Some(MessageData {
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
                        config_hash: invalid_utf.clone(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(ConfigMap::new(HashMap::from([(
                    "my-config".to_string(),
                    "enable_proces_metrics: true".to_string(),
                )]))),
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new("".to_string());
                    expected_hash.fail(
                        "Invalid hash: invalid utf-8 sequence of 1 bytes from index 0".into(),
                    );
                    expected_hash
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
