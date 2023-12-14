use crate::config::super_agent_configs::{AgentID, OpAMPClientConfig};
use crate::event::channel::EventPublisher;
use crate::event::event::OpAMPEvent;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::{
    build_http_client, OpAMPClientBuilder, OpAMPClientBuilderError,
};
use crate::sub_agent::opamp::remote_config_publisher::SubAgentRemoteConfigPublisher;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::opamp::client_builder::SuperAgentOpAMPHttpBuilder;
use crate::utils::time::get_sys_time_nano;
use futures::executor::block_on;
use opamp_client::http::{HttpClientReqwest, NotStartedHttpClient, StartedHttpClient};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{Client, NotStartedClient};

/// OpAMPBuilderCfg
pub struct SubAgentOpAMPHttpBuilder {
    config: OpAMPClientConfig,
}

impl SubAgentOpAMPHttpBuilder {
    pub fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }
}

impl<'a> From<&'a SuperAgentOpAMPHttpBuilder> for SubAgentOpAMPHttpBuilder {
    fn from(value: &'a SuperAgentOpAMPHttpBuilder) -> Self {
        SubAgentOpAMPHttpBuilder {
            config: value.config().clone(),
        }
    }
}

impl OpAMPClientBuilder<SubAgentCallbacks> for SubAgentOpAMPHttpBuilder {
    type Client = StartedHttpClient<SubAgentCallbacks, HttpClientReqwest>;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config)?;
        let remote_config_publisher = SubAgentRemoteConfigPublisher::new(opamp_publisher);
        let callbacks = AgentCallbacks::new(agent_id, remote_config_publisher);

        let not_started_client = NotStartedHttpClient::new(http_client);
        let started_client = block_on(not_started_client.start(callbacks, start_settings))?;

        // TODO remove opamp health from here, it should be done outside
        // set OpAMP health
        block_on(started_client.set_health(AgentHealth {
            healthy: true,
            start_time_unix_nano: get_sys_time_nano()?,
            last_error: "".to_string(),
        }))?;

        Ok(started_client)
    }
}
