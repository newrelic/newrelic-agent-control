//! # Synchronous HTTP Client implementation for the OpAMP trait.

use std::sync::{Arc, RwLock};

use crate::{
    common::{
        clientstate::ClientSyncedState, message_processor::ProcessResult, nextmessage::NextMessage,
    },
    opamp::proto::{AgentCapabilities, AgentDisconnect, AgentToServer},
    operation::{callbacks::Callbacks, capabilities::Capabilities, settings::StartSettings},
    Client, ClientError, ClientResult,
};

use super::{http_client::HttpClient, sender::HttpSender};
use tracing::{debug, error};

/// An implementation of an OpAMP Synchronous Client using HTTP transport with HttpClient.
pub struct OpAMPHttpClient<C, L>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    sender: HttpSender<L>,
    callbacks: C,
    message: Arc<RwLock<NextMessage>>,
    synced_state: ClientSyncedState,
    capabilities: Capabilities,
}

/// OpAMPHttpClient synchronous HTTP implementation of the Client trait.
// If there is an error sending, the syncState should still be updated, we want it to be consistent
// with the agent status, and we leave the responsibility to the OpAMP server to call ReportFullState
// if it detects a gap on sequence numbers.
impl<C, L> OpAMPHttpClient<C, L>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    /// Initializes a new OpAMPHttpClient with the provided Callbacks, uid, Capabilities, and HttpClient.
    pub(super) fn new(
        callbacks: C,
        start_settings: StartSettings,
        http_client: L,
    ) -> ClientResult<Self> {
        Ok(Self {
            sender: HttpSender::new(http_client)?,
            callbacks,
            message: Arc::new(RwLock::new(NextMessage::new(AgentToServer {
                instance_uid: start_settings.instance_id,
                agent_description: Some(start_settings.agent_description.into()),
                capabilities: u64::from(start_settings.capabilities),
                ..Default::default()
            }))),
            synced_state: ClientSyncedState::default(),
            capabilities: start_settings.capabilities,
        })
    }

    /// Prompt the client to poll for updates and process messages.
    pub(super) fn poll(&self) -> ClientResult<()> {
        self.send_process()
    }
}

impl<C, L> OpAMPHttpClient<C, L>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    // A sync method that handles the process of sending an AgentToServer message,
    // receives a ServerToAgent message, and analyzes the resulting message to decide
    // whether to resend or remain synced.
    fn send_process(&self) -> ClientResult<()> {
        // send message
        let msg = self
            .message
            .write()
            .map_err(|_| ClientError::PoisonError)?
            .pop();
        let server_to_agent = self.sender.send(msg).map_err(|e| {
            let err_msg = e.to_string();
            self.callbacks.on_connect_failed(e.into());
            ClientError::ConnectFailedCallback(err_msg)
        })?;

        // We consider it connected if we receive 2XX status from the Server.
        self.callbacks.on_connect();

        tracing::trace!("Received payload: {}", server_to_agent);

        let result = crate::common::message_processor::process_message(
            server_to_agent,
            &self.callbacks,
            &self.synced_state,
            &self.capabilities,
            self.message.clone(),
        )?;

        // check if resend is needed
        match result {
            ProcessResult::NeedsResend => Ok(self.send_process()?),
            ProcessResult::Synced => Ok(()),
        }
    }
}

impl<C, L> Drop for OpAMPHttpClient<C, L>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    fn drop(&mut self) {
        // By OpAMP protocol, AgentDisconnect must be sent in the last message.
        match self.message.write() {
            Ok(mut next_message) => {
                let mut msg = next_message.pop();
                msg.agent_disconnect = Some(AgentDisconnect::default());

                let _ = self.sender.send(msg).inspect_err(|err| {
                    error!(%err, "Sending disconnect OpAMP message");
                });

                debug!("OpAMPHttpClient disconnected from server");
            }
            Err(err) => {
                error!(%err,"Assembling disconnect OpAMP message");
            }
        };
    }
}

impl<C, L> Client for OpAMPHttpClient<C, L>
where
    C: Callbacks + Send + Sync,
    L: HttpClient + Send + Sync,
{
    /// set_agent_description sets the agent description of the Agent.
    // It uses compression and will only modify the message if there is a change.
    fn set_agent_description(
        &self,
        description: crate::opamp::proto::AgentDescription,
    ) -> ClientResult<()> {
        if self
            .synced_state
            .agent_description_unchanged(&description)?
        {
            return Ok(());
        }
        self.synced_state
            .set_agent_description(description.clone())?;

        // update message
        self.message
            .write()
            .map_err(|_| ClientError::PoisonError)?
            .update(|msg| {
                msg.agent_description = Some(description);
            });

        debug!("sending AgentToServer with provided description");
        self.send_process()
    }

    /// set_health sets the health status of the Agent.
    // It uses compression and will only modify the message if there is a change.
    fn set_health(&self, health: crate::opamp::proto::ComponentHealth) -> ClientResult<()> {
        if !self
            .capabilities
            .has_capability(AgentCapabilities::ReportsHealth)
        {
            return Err(ClientError::UnsetHealthCapability);
        }

        if self.synced_state.health_unchanged(&health)? {
            return Ok(());
        }
        self.synced_state.set_health(health.clone())?;

        // update message
        self.message
            .write()
            .map_err(|_| ClientError::PoisonError)?
            .update(|msg| {
                msg.health = Some(health);
            });

        debug!("sending AgentToServer with provided health");
        self.send_process()
    }

    // update_effective_config fetches the current local effective config using
    // get_effective_config callback and sends it to the Server.
    fn update_effective_config(&self) -> ClientResult<()> {
        if !self
            .capabilities
            .has_capability(AgentCapabilities::ReportsEffectiveConfig)
        {
            return Err(ClientError::UnsetEffectConfigCapability);
        }

        let config = self
            .callbacks
            .get_effective_config()
            .map_err(|_| ClientError::EffectiveConfigError)?;

        // update message
        self.message
            .write()
            .map_err(|_| ClientError::PoisonError)?
            .update(|msg| {
                msg.effective_config = Some(config);
            });

        debug!("sending AgentToServer with fetched effective config");

        self.send_process()
    }

    // set_remote_config_status sends the status of the remote config
    // that was previously received from the Server
    // It uses compression and will only modify the message if there is a change.
    fn set_remote_config_status(
        &self,
        status: crate::opamp::proto::RemoteConfigStatus,
    ) -> ClientResult<()> {
        if !self
            .capabilities
            .has_capability(AgentCapabilities::ReportsRemoteConfig)
        {
            return Err(ClientError::UnsetRemoteConfigStatusCapability);
        }

        if self.synced_state.remote_config_status_unchanged(&status)? {
            return Ok(());
        }
        self.synced_state.set_remote_config_status(status.clone())?;

        self.message
            .write()
            .map_err(|_| ClientError::PoisonError)?
            .update(|msg| {
                msg.remote_config_status = Some(status);
            });

        debug!("sending AgentToServer with remote");
        self.send_process()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use http::StatusCode;
    use std::collections::HashMap;
    use tracing_test::traced_test;

    use super::super::http_client::test::{
        response_from_server_to_agent, MockHttpClientMockall, ResponseParts,
    };

    use crate::http::HttpClientError;
    use crate::opamp::proto::any_value::Value;
    use crate::opamp::proto::AgentDescription;
    use crate::opamp::proto::{
        AgentConfigFile, AgentConfigMap, AnyValue, ComponentHealth, EffectiveConfig, KeyValue,
        RemoteConfigStatus,
    };
    use crate::{
        capabilities,
        opamp::proto::ServerToAgent,
        operation::{callbacks::test::MockCallbacksMockall, settings::StartSettings},
    };

    #[test]
    fn unsuccessful_http_response() {
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(2).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts {
                    status: StatusCode::FORBIDDEN,
                    ..Default::default()
                },
            ))
        });
        let mut mock_callbacks = MockCallbacksMockall::new();
        mock_callbacks.should_on_connect_failed();

        let client =
            OpAMPHttpClient::new(mock_callbacks, StartSettings::default(), mock_client).unwrap();

        assert!(matches!(
            client.send_process().unwrap_err(),
            ClientError::ConnectFailedCallback(_)
        ));
    }

    #[test]
    fn reset_message_fields_after_send() {
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(5).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts::default(),
            ))
        });
        let mut mock_callbacks = MockCallbacksMockall::new();
        mock_callbacks.expect_on_connect().times(4).return_const(());
        mock_callbacks.expect_on_message().times(4).return_const(());
        mock_callbacks
            .expect_get_effective_config()
            .once()
            .returning(|| {
                Ok(EffectiveConfig {
                    config_map: Some(AgentConfigMap {
                        config_map: HashMap::from([(
                            "/test".to_string(),
                            AgentConfigFile::default(),
                        )]),
                    }),
                })
            });

        let settings = StartSettings {
            instance_id: "NOT_AN_UID".into(),
            capabilities: capabilities!(
                AgentCapabilities::ReportsEffectiveConfig,
                AgentCapabilities::ReportsHealth,
                AgentCapabilities::ReportsRemoteConfig
            ),
            ..Default::default()
        };

        let client = OpAMPHttpClient::new(mock_callbacks, settings, mock_client).unwrap();

        let random_value = KeyValue {
            key: "thing".to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue("thing_value".to_string())),
            }),
        };

        let result = client.set_agent_description(AgentDescription {
            identifying_attributes: vec![random_value.clone()],
            non_identifying_attributes: vec![random_value],
        });
        assert!(result.is_ok());

        let result = client.update_effective_config();
        assert!(result.is_ok());

        let result = client.set_health(ComponentHealth {
            healthy: false,
            start_time_unix_nano: 1689942447,
            last_error: "wow! what an error".to_string(),
            ..Default::default()
        });
        assert!(result.is_ok());

        let result = client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 2,
            error_message: "".to_string(),
        });
        assert!(result.is_ok());

        // this pop should return a current message with the attributes reset
        let message = client.message.write().unwrap().pop();

        assert_eq!(
            message.agent_description,
            AgentToServer::default().agent_description
        );
        assert_eq!(message.health, AgentToServer::default().health);
        assert_eq!(
            message.remote_config_status,
            AgentToServer::default().remote_config_status
        );
        assert_eq!(
            message.effective_config,
            AgentToServer::default().effective_config
        );

        assert_ne!(
            client.synced_state.agent_description().unwrap(),
            ClientSyncedState::default().agent_description().unwrap()
        );
        assert_ne!(
            client.synced_state.health().unwrap(),
            ClientSyncedState::default().health().unwrap()
        );
        assert_ne!(
            client.synced_state.remote_config_status().unwrap(),
            ClientSyncedState::default().remote_config_status().unwrap()
        );
    }

    // If there is an error sending, the syncState should still be updated, we want it to be consistent
    // with the agent status, and we leave the responsibility to the OpAMP server to call ReportFullState
    // if it detects a gap on sequence numbers.
    #[test]
    fn remote_config_status_should_still_sync_state_update_on_error() {
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().times(2).returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                ResponseParts {
                    status: StatusCode::FORBIDDEN,
                    ..Default::default()
                },
            ))
        });
        let mut mock_callbacks = MockCallbacksMockall::new();
        mock_callbacks.should_on_connect_failed();

        let settings = StartSettings {
            instance_id: "NOT_AN_UID".into(),
            capabilities: capabilities!(AgentCapabilities::ReportsRemoteConfig),
            ..Default::default()
        };

        let client = OpAMPHttpClient::new(mock_callbacks, settings, mock_client).unwrap();

        let remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: vec![],
            status: 2,
            error_message: "".to_string(),
        };
        let result = client.set_remote_config_status(remote_config_status.clone());

        assert!(matches!(
            result.unwrap_err(),
            ClientError::ConnectFailedCallback(_)
        ));

        assert_eq!(
            client.synced_state.remote_config_status().unwrap().unwrap(),
            remote_config_status
        );
    }

    #[traced_test]
    #[test]
    fn test_drop_success() {
        let mut mock_client = MockHttpClientMockall::new();
        mock_client.expect_post().once().returning(|_| {
            Ok(response_from_server_to_agent(
                &ServerToAgent::default(),
                Default::default(),
            ))
        });

        let client = OpAMPHttpClient::new(
            MockCallbacksMockall::new(),
            StartSettings::default(),
            mock_client,
        )
        .unwrap();

        drop(client);

        assert!(logs_contain("OpAMPHttpClient disconnected from server"));
    }
    #[traced_test]
    #[test]
    fn test_fail_to_drop() {
        let mut mock_client = MockHttpClientMockall::new();
        mock_client
            .expect_post()
            .once()
            .returning(|_| Err(HttpClientError::TransportError("some error".to_string())));

        let client = OpAMPHttpClient::new(
            MockCallbacksMockall::new(),
            StartSettings::default(),
            mock_client,
        )
        .unwrap();

        drop(client);

        assert!(logs_contain("some error"));
    }
}
