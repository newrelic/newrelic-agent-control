use std::collections::HashMap;

use futures::executor::block_on;
use opamp_client::{
    opamp::proto::AgentHealth,
    operation::settings::{AgentDescription, DescriptionValueType, StartSettings},
};
use tracing::info;

use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::sub_agent::error::SubAgentError;
use crate::{
    config::super_agent_configs::{AgentID, AgentTypeFQN},
    context::Context,
    opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    super_agent::super_agent::SuperAgentEvent,
    utils::time::get_sys_time_nano,
};

pub fn build_opamp_and_start_client<OpAMPBuilder, InstanceIdGetter>(
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_builder: Option<&OpAMPBuilder>,
    instance_id_getter: &InstanceIdGetter,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    InstanceIdGetter: InstanceIDGetter,
{
    match opamp_builder {
        Some(builder) => {
            let start_settings = start_settings(
                instance_id_getter.get(&agent_id)?.to_string(),
                agent_type,
                non_identifying_attributes,
            );

            Ok(Some(builder.build_and_start(
                ctx,
                agent_id,
                start_settings,
            )?))
        }
        None => Ok(None),
    }
}

/// Builds and start an OpAMP client when a builder is provided.
pub fn start_opamp_client<O: OpAMPClientBuilder>(
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_builder: Option<&O>,
    agent_id: AgentID,
    start_settings: StartSettings,
) -> Result<Option<O::Client>, OpAMPClientBuilderError> {
    match opamp_builder {
        Some(builder) => Ok(Some(builder.build_and_start(
            ctx,
            agent_id,
            start_settings,
        )?)),
        None => Ok(None),
    }
}

/// Builds the OpAMP StartSettings corresponding to the provided arguments for any sub agent.
pub fn start_settings(
    instance_id: String,
    agent_fqn: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> StartSettings {
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
            non_identifying_attributes,
        },
    }
}

/// Stops an started OpAMP client.
pub fn stop_opamp_client<C: opamp_client::StartedClient>(
    client: Option<C>,
    agent_id: &AgentID,
) -> Result<(), SubAgentError> {
    if let Some(client) = client {
        info!(
            "Stopping OpAMP client for supervised agent type: {}",
            agent_id
        );
        block_on(client.set_health(AgentHealth {
            healthy: false,
            start_time_unix_nano: get_sys_time_nano()?,
            last_error: "".to_string(),
        }))?;
        block_on(client.stop())?;
    }
    Ok(())
}
