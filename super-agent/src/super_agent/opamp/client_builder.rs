use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::{
    build_http_client, OpAMPClientBuilder, OpAMPClientBuilderError,
};
use crate::super_agent::config::{AgentID, OpAMPClientConfig};
use crate::super_agent::opamp::remote_config_publisher::SuperAgentRemoteConfigPublisher;
use crate::super_agent::SuperAgentCallbacks;
use crate::utils::time::get_sys_time_nano;
use opamp_client::http::{HttpClientUreq, NotStartedHttpClient, StartedHttpClient};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{Client, NotStartedClient};
use tracing::instrument;

/// OpAMPBuilderCfg
#[derive(Debug)]
pub struct SuperAgentOpAMPHttpBuilder {
    config: OpAMPClientConfig,
}

impl SuperAgentOpAMPHttpBuilder {
    pub fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &OpAMPClientConfig {
        &self.config
    }
}

impl OpAMPClientBuilder<SuperAgentCallbacks> for SuperAgentOpAMPHttpBuilder {
    type Client = StartedHttpClient<SuperAgentCallbacks, HttpClientUreq>;

    #[instrument]
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config)?;
        let remote_config_publisher = SuperAgentRemoteConfigPublisher::new(opamp_publisher);
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
