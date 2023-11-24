use crate::config::super_agent_configs::{AgentID, OpAMPClientConfig};
use crate::context::Context;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::{
    build_http_client, OpAMPClientBuilder, OpAMPClientBuilderError,
};
use crate::super_agent::opamp::remote_config_publisher::SuperAgentRemoteConfigPublisher;
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::utils::time::get_sys_time_nano;
use futures::executor::block_on;
use opamp_client::http::{HttpClientReqwest, NotStartedHttpClient, StartedHttpClient};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{Client, NotStartedClient};

/// OpAMPBuilderCfg
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

impl OpAMPClientBuilder for SuperAgentOpAMPHttpBuilder {
    type Client =
        StartedHttpClient<AgentCallbacks<SuperAgentRemoteConfigPublisher>, HttpClientReqwest>;
    fn build_and_start(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config)?;
        let remote_config_publisher = SuperAgentRemoteConfigPublisher::new(ctx);
        let callbacks = AgentCallbacks::new(agent_id, remote_config_publisher);
        let not_started_client = NotStartedHttpClient::new(callbacks, start_settings, http_client)?;
        let started_client = block_on(not_started_client.start())?;
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
