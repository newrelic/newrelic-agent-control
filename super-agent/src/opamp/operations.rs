use std::collections::HashMap;

use crate::{
    config::super_agent_configs::{AgentID, AgentTypeFQN},
    event::{channel::EventPublisher, OpAMPEvent},
    sub_agent::error::SubAgentError,
    utils::time::get_sys_time_nano,
};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::{
    operation::{
        callbacks::Callbacks,
        settings::{AgentDescription, DescriptionValueType, StartSettings},
    },
    StartedClient,
};
use tracing::info;

use super::{
    client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    instance_id::getter::InstanceIDGetter,
};

pub fn build_opamp_and_start_client<CB, OB, IG>(
    opamp_publisher: EventPublisher<OpAMPEvent>,
    opamp_builder: Option<&OB>,
    instance_id_getter: &IG,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<Option<OB::Client>, OpAMPClientBuilderError>
where
    CB: Callbacks,
    OB: OpAMPClientBuilder<CB>,
    IG: InstanceIDGetter,
{
    match opamp_builder {
        Some(builder) => {
            let start_settings = start_settings(
                instance_id_getter.get(&agent_id)?.to_string(),
                agent_type,
                non_identifying_attributes,
            );

            Ok(Some(builder.build_and_start(
                opamp_publisher,
                agent_id,
                start_settings,
            )?))
        }
        None => Ok(None),
    }
}

/// Builds and start an OpAMP client when a builder is provided.
pub fn start_opamp_client<CB: Callbacks, O: OpAMPClientBuilder<CB>>(
    opamp_publisher: EventPublisher<OpAMPEvent>,
    opamp_builder: Option<&O>,
    agent_id: AgentID,
    start_settings: StartSettings,
) -> Result<Option<O::Client>, OpAMPClientBuilderError> {
    match opamp_builder {
        Some(builder) => Ok(Some(builder.build_and_start(
            opamp_publisher,
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
pub fn stop_opamp_client<CB: Callbacks, C: StartedClient<CB>>(
    client: Option<C>,
    agent_id: &AgentID,
) -> Result<(), SubAgentError> {
    if let Some(client) = client {
        info!(
            "Stopping OpAMP client for supervised agent type: {}",
            agent_id
        );
        crate::runtime::runtime().block_on(client.set_health(AgentHealth {
            healthy: false,
            start_time_unix_nano: get_sys_time_nano()?,
            last_error: "".to_string(),
        }))?;
        crate::runtime::runtime().block_on(client.stop())?;
    }
    Ok(())
}
