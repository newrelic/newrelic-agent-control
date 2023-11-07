use crate::config::super_agent_configs::{AgentID, AgentTypeFQN, OpAMPClientConfig};
use crate::context::Context;
use crate::opamp::client_builder::{
    build_http_client, OpAMPClientBuilder, OpAMPClientBuilderError,
};
use crate::sub_agent::callbacks::AgentCallbacks as SubAgentCallbacks;
use crate::super_agent::callbacks::AgentCallbacks as SuperAgentCallbacks;
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::utils::time::get_sys_time_nano;
use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::http::{HttpClientReqwest, NotStartedHttpClient, StartedHttpClient};
use opamp_client::opamp::proto::{AgentCapabilities, AgentHealth};
use opamp_client::operation::settings::{AgentDescription, StartSettings};
use opamp_client::{capabilities, Client, NotStartedClient};
use std::collections::HashMap;

/// OpAMPBuilderCfg
pub struct SuperAgentOpAMPHttpBuilder {
    config: OpAMPClientConfig,
}

impl SuperAgentOpAMPHttpBuilder {
    pub fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }
}

impl OpAMPClientBuilder for SuperAgentOpAMPHttpBuilder {
    type Client = StartedHttpClient<SuperAgentCallbacks, HttpClientReqwest>;
    fn build_and_start(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config)?;
        let callbacks = SuperAgentCallbacks::new(ctx, agent_id);
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
            config: value.config.clone(),
        }
    }
}

impl OpAMPClientBuilder for SubAgentOpAMPHttpBuilder {
    type Client = StartedHttpClient<SubAgentCallbacks, HttpClientReqwest>;
    fn build_and_start(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = build_http_client(&self.config)?;
        let callbacks = SubAgentCallbacks::new(ctx, agent_id);
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

pub(super) fn build_opamp_and_start_client<OpAMPBuilder, InstanceIdGetter>(
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_builder: Option<&OpAMPBuilder>,
    instance_id_getter: &InstanceIdGetter,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    InstanceIdGetter: InstanceIDGetter,
{
    match opamp_builder {
        Some(builder) => {
            let start_settings = start_settings(instance_id_getter.get(&agent_id), agent_type);

            Ok(Some(builder.build_and_start(
                ctx,
                agent_id,
                start_settings,
            )?))
        }
        None => Ok(None),
    }
}

fn start_settings(instance_id: String, agent_fqn: &AgentTypeFQN) -> StartSettings {
    StartSettings {
        instance_id,
        capabilities: agent_fqn.get_capabilities(),
        agent_description: AgentDescription {
            identifying_attributes: HashMap::from([
                ("service.name".to_string(), agent_fqn.name().into()),
                (
                    "service.namespace".to_string(),
                    agent_fqn.namespace().into(),
                ),
                ("service.version".to_string(), agent_fqn.version().into()),
            ]),
            non_identifying_attributes: HashMap::from([(
                "host.name".to_string(),
                get_hostname().into(),
            )]),
        },
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}
