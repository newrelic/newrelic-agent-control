use crate::auth::token::TokenRetriever;
use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::auth_http_client::AuthHttpClient;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::{
    build_http_client, OpAMPClientBuilder, OpAMPClientBuilderError,
};
use crate::sub_agent::opamp::remote_config_publisher::SubAgentRemoteConfigPublisher;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::{AgentID, OpAMPClientConfig};
use crate::super_agent::opamp::client_builder::SuperAgentOpAMPHttpBuilder;
use crate::utils::time::get_sys_time_nano;
use opamp_client::http::{HttpClientUreq, NotStartedHttpClient, StartedHttpClient};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{Client, NotStartedClient};
use std::sync::Arc;

/// OpAMPBuilderCfg
pub struct SubAgentOpAMPHttpBuilder<T> {
    config: OpAMPClientConfig,
    token_retriever: Arc<T>,
}

impl<T> SubAgentOpAMPHttpBuilder<T>
where
    T: TokenRetriever,
{
    pub fn new(config: OpAMPClientConfig, token_retriever: Arc<T>) -> Self {
        Self {
            config,
            token_retriever,
        }
    }
}

impl<'a, T> From<&'a SuperAgentOpAMPHttpBuilder<T>> for SubAgentOpAMPHttpBuilder<T>
where
    T: TokenRetriever,
{
    fn from(value: &'a SuperAgentOpAMPHttpBuilder<T>) -> Self {
        SubAgentOpAMPHttpBuilder {
            config: value.config().clone(),
            token_retriever: value.token_retriever(),
        }
    }
}

impl<T> OpAMPClientBuilder<SubAgentCallbacks> for SubAgentOpAMPHttpBuilder<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    type Client = StartedHttpClient<SubAgentCallbacks, AuthHttpClient<T>>;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config, self.token_retriever.clone())?;
        let remote_config_publisher = SubAgentRemoteConfigPublisher::new(opamp_publisher);
        let callbacks = AgentCallbacks::new(agent_id, remote_config_publisher);

        let not_started_client = NotStartedHttpClient::new(http_client);
        let started_client = not_started_client.start(callbacks, start_settings)?;

        // TODO remove opamp health from here, it should be done outside
        // set OpAMP health
        started_client.set_health(AgentHealth {
            healthy: true,
            start_time_unix_nano: get_sys_time_nano()?,
            last_error: "".to_string(),
        })?;

        Ok(started_client)
    }
}
