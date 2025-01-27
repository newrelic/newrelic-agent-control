use super::effective_config::{error::EffectiveConfigError, loader::EffectiveConfigLoader};
use crate::opamp::remote_config::signature::Signatures;
use crate::opamp::remote_config::{hash::Hash, ConfigurationMap, RemoteConfig};
use crate::{agent_control::config::AgentID, opamp::remote_config::RemoteConfigError};
use crate::{
    event::{
        channel::{EventPublisher, EventPublisherError},
        OpAMPEvent,
    },
    opamp::remote_config::signature::SignatureError,
};
use opamp_client::{
    error::ConnectionError,
    error::ConnectionError::HTTPClientError,
    http::HttpClientError,
    opamp::proto::{
        AgentRemoteConfig, CustomMessage, EffectiveConfig, OpAmpConnectionSettings,
        ServerErrorResponse, ServerToAgentCommand,
    },
    operation::callbacks::{Callbacks, MessageData},
};
use std::string::FromUtf8Error;
use thiserror::Error;
use tracing::{debug, error, trace};
use HttpClientError::UnsuccessfulResponse;

#[derive(Debug, Error)]
pub enum AgentCallbacksError {
    #[error("deserialization error: `{0}`")]
    DeserializationError(#[from] RemoteConfigError),

    #[error("Invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] FromUtf8Error),

    #[error("unable to publish OpAMP event")]
    PublishEventError(#[from] EventPublisherError),

    #[error("unable to get effective config: `{0}`")]
    EffectiveConfigError(#[from] EffectiveConfigError),
}

/// This component implements the OpAMP client callbacks process the messages and publish events on `crate::event::OpAMPEvent`.
pub struct AgentCallbacks<C>
where
    C: EffectiveConfigLoader,
{
    agent_id: AgentID,
    publisher: EventPublisher<OpAMPEvent>,
    effective_config_loader: C,
}

impl<C> AgentCallbacks<C>
where
    C: EffectiveConfigLoader,
{
    pub fn new(
        agent_id: AgentID,
        publisher: EventPublisher<OpAMPEvent>,
        effective_config_loader: C,
    ) -> Self {
        Self {
            agent_id,
            publisher,
            effective_config_loader,
        }
    }

    /// Assembles a `RemoteConfig` from the OpAMP message and publish the `crate::event::OpAMPEvent::RemoteConfigReceived`.
    fn process_remote_config(
        &self,
        msg_remote_config: AgentRemoteConfig,
        custom_message: Option<CustomMessage>,
    ) -> Result<(), AgentCallbacksError> {
        trace!(
            agent_id = self.agent_id.to_string(),
            "OpAMP remote config message received"
        );

        let mut hash = match String::from_utf8(msg_remote_config.config_hash) {
            Ok(hash) => Hash::new(hash.to_string()),
            Err(err) => {
                // the hash must be created to keep track of the failing remote config.
                let mut hash = Hash::new(String::new());
                hash.fail(format!("Invalid hash: {}", err));
                hash
            }
        };

        let config_map: Option<ConfigurationMap> = match msg_remote_config.config {
            Some(msg_config_map) => msg_config_map
                .try_into()
                .inspect_err(|err: &RemoteConfigError| {
                    hash.fail(format!("Invalid remote config format: {}", err))
                })
                .ok(),
            None => {
                hash.fail("Config missing".into());
                None
            }
        };

        let maybe_config_signature =
            custom_message.and_then(
                |custom_message|  Signatures::try_from(&custom_message).inspect_err(|err|match err {
                    SignatureError::InvalidCapability |SignatureError::InvalidType  => {
                        debug!(%self.agent_id, "custom message doesn't contain a valid config signature: {:?}", custom_message);
                    },
                    SignatureError::InvalidData(err) => {
                        error!(%self.agent_id, %err, "parsing config signature message: {:?}", custom_message);
                        hash.fail(format!("Invalid remote config signature format: {}", err));
                    },
                    SignatureError::UnsupportedAlgorithm(err) => {
                        error!(%self.agent_id, %err, "unsupported signature algorithm: {:?}", custom_message);
                        hash.fail(format!("Unsupported signature algorithm: {}", err));
                    }
                }).ok()
            );

        let mut remote_config = RemoteConfig::new(self.agent_id.clone(), hash, config_map);
        if let Some(config_signature) = maybe_config_signature {
            remote_config = remote_config.with_signature(config_signature);
        }

        trace!(
            agent_id = self.agent_id.to_string(),
            "remote config received: {:?}",
            &remote_config
        );

        Ok(self
            .publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))?)
    }

    fn publish_on_connect(&self) {
        let _ = self
            .publisher
            .publish(OpAMPEvent::Connected)
            .inspect_err(|err| {
                error!(
                    error_msg = %err,
                    "error publishing opamp_event.connected"
                )
            });
    }

    fn publish_on_connect_failed(&self, err: &ConnectionError) {
        let (code, reason) = if let HTTPClientError(UnsuccessfulResponse(code, reason)) = &err {
            (Some(*code), reason.clone())
        } else {
            (None, err.to_string())
        };

        let _ = self
            .publisher
            .publish(OpAMPEvent::ConnectFailed(code, reason))
            .inspect_err(|err| {
                error!(
                    %self.agent_id,
                    error_msg = %err,
                    "error publishing opamp_event.connected"
                )
            });
    }
}

impl<C> Callbacks for AgentCallbacks<C>
where
    C: EffectiveConfigLoader,
{
    type Error = AgentCallbacksError;

    fn on_connect(&self) {
        self.publish_on_connect();
    }

    fn on_connect_failed(&self, err: ConnectionError) {
        log_connection_error(&err, self.agent_id.clone());
        self.publish_on_connect_failed(&err);
    }

    fn on_error(&self, _err: ServerErrorResponse) {}

    fn on_message(&self, msg: MessageData) {
        trace!(agent_id = %self.agent_id, "opamp message received: {:?}", msg);
        if let Some(msg_remote_config) = msg.remote_config {
            match self.process_remote_config(msg_remote_config, msg.custom_message) {
                Ok(_) => trace!(agent_id = %self.agent_id, "on message ok"),
                Err(e) => error!(agent_id = %self.agent_id, err = %e, "processing OpAMP message"),
            };
        } else {
            trace!(
                agent_id = %self.agent_id,
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
        debug!(
            agent_id = %self.agent_id,
            "OpAMP get effective config");

        let effective_config = self
            .effective_config_loader
            .load()
            .map_err(EffectiveConfigError::Loader)?;

        // Not printing the effective config in case it contains sensitive info
        debug!(
            agent_id = %self.agent_id,
            "OpAMP effective config loaded"
        );

        Ok(effective_config.into())
    }
}

fn log_connection_error(err: &ConnectionError, agent_id: AgentID) {
    // Check if the error comes from receiving an undesired HTTP status code
    if let HTTPClientError(UnsuccessfulResponse(http_code, http_reason)) = &err {
        let reason = match http_code {
            400 => "The request was malformed",
            401 => "Check for missing or invalid license key",
            403 => "The account provided is not allowed to use this resource",
            404 => "The requested resource was not found",
            415 => "Content-Type or Content-Encoding for the HTTP request was wrong",
            500 => "Server-side problem",
            _ => "Reasons unknown",
        };
        error!(%agent_id, http_code, http_reason, reason,"OpAMP HTTP connection error");
    } else {
        error!(
            %agent_id,
            reason = err.to_string(),
            "OpAMP HTTP connection error"
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::event::channel::pub_sub;
    use crate::event::OpAMPEvent;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::signature::{
        ED25519, SIGNATURE_CUSTOM_CAPABILITY, SIGNATURE_CUSTOM_MESSAGE_TYPE,
    };
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, AgentRemoteConfig};
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_connect() {
        let (event_publisher, event_consumer) = pub_sub();
        let effective_config_loader = MockEffectiveConfigLoaderMock::new();

        let callbacks = AgentCallbacks::new(
            AgentID::new("agent").unwrap(),
            event_publisher,
            effective_config_loader,
        );

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
        let effective_config_loader = MockEffectiveConfigLoaderMock::new();

        let callbacks = AgentCallbacks::new(
            AgentID::new("agent").unwrap(),
            event_publisher,
            effective_config_loader,
        );

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
        assert_eq!(
            event,
            OpAMPEvent::ConnectFailed(Some(status), reason.to_string())
        );
        assert!(event_consumer.as_ref().is_empty());

        // When an error without status code and reason is received
        callbacks.on_connect_failed(ConnectionError::HTTPClientError(
            HttpClientError::TransportError("Some transport error".to_string()),
        ));

        let _ = event_consumer
            .as_ref()
            .recv_timeout(Duration::from_millis(1))
            .expect("transport error event should be received too");
    }

    #[test]
    fn test_remote_config() {
        let valid_hash = "hash";
        let invalid_utf = vec![128, 129];
        let (valid_remote_config_map, expected_remote_config_map) = (
            AgentConfigMap {
                config_map: HashMap::from([(
                    "my-config".to_string(),
                    AgentConfigFile {
                        body: "enable_proces_metrics: true".as_bytes().to_vec(),
                        content_type: "".to_string(),
                    },
                )]),
            },
            ConfigurationMap::new(HashMap::from([(
                "my-config".to_string(),
                "enable_proces_metrics: true".to_string(),
            )])),
        );

        struct TestCase {
            name: &'static str,
            opamp_msg: Option<MessageData>, // using option here to allow taking the ownership of the MessageData which cannot be cloned.
            expected_remote_config_hash: Hash,
            expected_remote_config_config_map: Option<ConfigurationMap>,
            expected_signature: Option<Signatures>,
        }
        impl TestCase {
            fn run(mut self) {
                let agent_id = AgentID::new("an-agent-id").unwrap();

                let (event_publisher, event_consumer) = pub_sub();
                let effective_config_loader = MockEffectiveConfigLoaderMock::new();

                let callbacks =
                    AgentCallbacks::new(agent_id.clone(), event_publisher, effective_config_loader);

                callbacks.on_message(self.opamp_msg.take().unwrap());

                let event = event_consumer
                    .as_ref()
                    .recv_timeout(Duration::from_millis(1))
                    .unwrap();

                let mut remote_config = RemoteConfig::new(
                    agent_id.clone(),
                    self.expected_remote_config_hash.clone(),
                    self.expected_remote_config_config_map.clone(),
                );
                if let Some(signature) = self.expected_signature {
                    remote_config = remote_config.with_signature(signature);
                }

                let expected_event = OpAMPEvent::RemoteConfigReceived(remote_config);

                assert_eq!(event, expected_event, "test case: {}", self.name);

                assert!(event_consumer.as_ref().is_empty());
            }
        }
        let test_cases = vec![
            TestCase {
                name: "with valid values",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: Hash::new(valid_hash.to_string()),
                expected_signature: None,
            },
            TestCase {
                name: "with invalid values",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(AgentConfigMap {
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
                        "Invalid remote config format: invalid UTF-8 sequence: `invalid utf-8 sequence of 1 bytes from index 0`".into(),
                    );
                    expected_hash
                },
                expected_signature: None,
            },
            TestCase {
                name: "with missing config and valid hash",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
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
                expected_signature: None,
            },
            TestCase {
                name: "with missing config and invalid hash",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
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
                expected_signature: None,
            },
            TestCase {
                name: "with valid config and invalid hash",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: invalid_utf.clone(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new("".to_string());
                    expected_hash.fail(
                        "Invalid hash: invalid utf-8 sequence of 1 bytes from index 0".into(),
                    );
                    expected_hash
                },
                expected_signature: None,
            },
            TestCase {
                name: "with valid config and valid signature",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    custom_message: Some(CustomMessage {
                        capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                        r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                        data: r#"{
                            "unique": [{
                                "signature": "fake config",
                                "signingAlgorithm": "ED25519",
                                "keyId": "fake keyid"
                            }]
                        }"#
                        .as_bytes()
                        .to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: Hash::new(valid_hash.to_string()),
                expected_signature: Some(Signatures::new_unique(
                    "fake config",
                    ED25519,
                    "fake keyid",
                )),
            },
            TestCase {
                name: "with valid config and invalid signature type",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    custom_message: Some(CustomMessage {
                        capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                        r#type: "unsupported_type".to_string(),
                        data: r#"{
                            "unique": [{
                                "signature": "fake config",
                                "signingAlgorithm": "ED25519",
                                "keyId": "fake keyid"
                            }]
                        }"#
                        .as_bytes()
                        .to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: Hash::new(valid_hash.to_string()),
                expected_signature: None,
            },
            TestCase {
                name: "with valid config and invalid signature capability",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    custom_message: Some(CustomMessage {
                        capability: "unsupported.capability".to_string(),
                        r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                        data: r#"{
                            "unique": [{
                                "signature": "fake config",
                                "signingAlgorithm": "ED25519",
                                "keyId": "fake keyid"
                            }]
                        }"#
                        .as_bytes()
                        .to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: Hash::new(valid_hash.to_string()),
                expected_signature: None,
            },
            TestCase {
                name: "with valid config and invalid signature data",
                opamp_msg: Some(MessageData {
                    remote_config: Some(AgentRemoteConfig {
                        config: Some(valid_remote_config_map.clone()),
                        config_hash: valid_hash.as_bytes().to_vec(),
                    }),
                    custom_message: Some(CustomMessage {
                        capability: SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                        r#type: SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                        data: "invalid signature".as_bytes().to_vec(),
                    }),
                    ..Default::default()
                }),
                expected_remote_config_config_map: Some(expected_remote_config_map.clone()),
                expected_remote_config_hash: {
                    let mut expected_hash = Hash::new(valid_hash.to_string());
                    expected_hash.fail(
                        "Invalid remote config signature format: expected value at line 1 column 1"
                            .into(),
                    );
                    expected_hash
                },
                expected_signature: None,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_get_effective_config() {
        let (event_publisher, _event_consumer) = pub_sub();
        let mut effective_config_loader = MockEffectiveConfigLoaderMock::new();

        effective_config_loader
            .expect_load()
            .once()
            .returning(|| Ok(ConfigurationMap::default()));

        let callbacks = AgentCallbacks::new(
            AgentID::new("agent").unwrap(),
            event_publisher,
            effective_config_loader,
        );

        let actual = callbacks.get_effective_config().unwrap();

        let expected = EffectiveConfig {
            config_map: Some(AgentConfigMap::default()),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_get_effective_config_err() {
        let (event_publisher, _event_consumer) = pub_sub();
        let mut effective_config_loader = MockEffectiveConfigLoaderMock::new();

        effective_config_loader
            .expect_load()
            .once()
            .returning(|| Err("loader error".to_string().into()));

        let callbacks = AgentCallbacks::new(
            AgentID::new("agent").unwrap(),
            event_publisher,
            effective_config_loader,
        );

        let actual = callbacks.get_effective_config().unwrap_err();

        assert!(matches!(
            actual,
            AgentCallbacksError::EffectiveConfigError(EffectiveConfigError::Loader(_))
        ));
    }
}
