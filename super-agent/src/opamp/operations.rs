use std::collections::HashMap;

use crate::super_agent::defaults::PARENT_AGENT_ID_ATTRIBUTE_KEY;
use crate::{
    event::{
        channel::{pub_sub, EventConsumer, EventPublisher},
        OpAMPEvent,
    },
    sub_agent::error::SubAgentError,
    super_agent::config::{AgentID, AgentTypeFQN},
};
use opamp_client::{
    operation::{
        callbacks::Callbacks,
        settings::{AgentDescription, DescriptionValueType, StartSettings},
    },
    StartedClient,
};
use tracing::info;

use super::instance_id::InstanceIDGetter;
use super::{
    client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    instance_id::getter::IDGetter,
};

pub fn build_sub_agent_opamp<CB, OB, IG>(
    opamp_builder: &OB,
    instance_id_getter: &IG,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
    mut non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<(OB::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
where
    CB: Callbacks,
    OB: OpAMPClientBuilder<CB>,
    IG: IDGetter,
{
    let super_agent_id = AgentID::new_super_agent_id();
    let parent_instance_id = instance_id_getter.get(&super_agent_id)?.to_string();

    non_identifying_attributes.insert(
        PARENT_AGENT_ID_ATTRIBUTE_KEY().to_string(),
        DescriptionValueType::String(parent_instance_id),
    );

    build_opamp_with_channel(
        opamp_builder,
        instance_id_getter,
        agent_id.clone(),
        agent_type,
        non_identifying_attributes,
    )
}

pub fn build_opamp_with_channel<CB, OB, IG>(
    opamp_builder: &OB,
    instance_id_getter: &IG,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<(OB::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
where
    CB: Callbacks,
    OB: OpAMPClientBuilder<CB>,
    IG: IDGetter,
{
    let (tx, rx) = pub_sub();
    let client = build_opamp_and_start_client(
        tx,
        opamp_builder,
        instance_id_getter,
        agent_id,
        agent_type,
        non_identifying_attributes,
    )?;
    Ok((client, rx))
}

pub fn build_opamp_and_start_client<CB, OB, IG>(
    opamp_publisher: EventPublisher<OpAMPEvent>,
    opamp_builder: &OB,
    instance_id_getter: &IG,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<OB::Client, OpAMPClientBuilderError>
where
    CB: Callbacks,
    OB: OpAMPClientBuilder<CB>,
    IG: IDGetter,
{
    let start_settings = start_settings(
        instance_id_getter.get(&agent_id)?,
        agent_type,
        non_identifying_attributes,
    );
    let started_opamp_client =
        opamp_builder.build_and_start(opamp_publisher, agent_id, start_settings)?;

    Ok(started_opamp_client)
}

/// Builds the OpAMP StartSettings corresponding to the provided arguments for any sub agent.
pub fn start_settings(
    instance_id: InstanceIDGetter,
    agent_fqn: &AgentTypeFQN,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> StartSettings {
    StartSettings {
        instance_id: instance_id.into(),
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
        // TODO We should call disconnect here as this means a graceful shutdown
        client.stop()?;
    }
    Ok(())
}
