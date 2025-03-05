use super::instance_id::InstanceID;
use super::{
    client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    instance_id::getter::InstanceIDGetter,
};
use crate::agent_control::defaults::{
    default_capabilities, OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE,
    PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::sub_agent::identity::AgentIdentity;
use crate::{
    agent_control::agent_id::AgentID,
    agent_control::config::AgentTypeFQN,
    event::{
        channel::{pub_sub, EventConsumer},
        OpAMPEvent,
    },
    sub_agent::error::SubAgentError,
};
use opamp_client::{
    operation::settings::{AgentDescription, DescriptionValueType, StartSettings},
    StartedClient,
};
use std::collections::HashMap;
use tracing::info;

pub fn build_sub_agent_opamp<OB, IG>(
    opamp_builder: &OB,
    instance_id_getter: &IG,
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    mut non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<(OB::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
where
    OB: OpAMPClientBuilder,
    IG: InstanceIDGetter,
{
    let agent_control_id = AgentID::new_agent_control_id();
    let parent_instance_id = instance_id_getter.get(&agent_control_id)?;

    non_identifying_attributes.insert(
        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::Bytes(parent_instance_id.into()),
    );

    build_opamp_with_channel(
        opamp_builder,
        instance_id_getter,
        agent_identity,
        additional_identifying_attributes,
        non_identifying_attributes,
    )
}

pub fn build_opamp_with_channel<OB, IG>(
    opamp_builder: &OB,
    instance_id_getter: &IG,
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<(OB::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
where
    OB: OpAMPClientBuilder,
    IG: InstanceIDGetter,
{
    let (opamp_publisher, opamp_consumer) = pub_sub();
    let start_settings = start_settings(
        instance_id_getter.get(&agent_identity.id)?,
        &agent_identity.fqn,
        additional_identifying_attributes,
        non_identifying_attributes,
    );
    let started_opamp_client = opamp_builder.build_and_start(
        opamp_publisher,
        agent_identity.id.clone(),
        start_settings,
    )?;

    Ok((started_opamp_client, opamp_consumer))
}

/// Builds the OpAMP StartSettings corresponding to the provided arguments for any sub agent and agent control.
pub fn start_settings(
    instance_id: InstanceID,
    agent_fqn: &AgentTypeID,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> StartSettings {
    let mut identifying_attributes = HashMap::from([
        (OPAMP_SERVICE_NAME.to_string(), agent_fqn.name().into()),
        (
            OPAMP_SERVICE_NAMESPACE.to_string(),
            agent_fqn.namespace().into(),
        ),
    ]);

    identifying_attributes.extend(additional_identifying_attributes);

    StartSettings {
        instance_uid: instance_id.into(),
        capabilities: default_capabilities(),
        custom_capabilities: agent_fqn.get_custom_capabilities(),
        agent_description: AgentDescription {
            identifying_attributes,
            non_identifying_attributes,
        },
    }
}

/// Stops an started OpAMP client.
pub fn stop_opamp_client<C: StartedClient>(
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
