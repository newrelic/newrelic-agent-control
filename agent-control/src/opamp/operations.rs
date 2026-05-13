use super::{client_builder::OpAMPClientBuilderError, instance_id::getter::InstanceIDGetter};
use crate::agent_control::defaults::{
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SUPERVISOR_KEY,
    PARENT_AGENT_ID_ATTRIBUTE_KEY, default_capabilities, default_custom_capabilities,
};
use crate::sub_agent::identity::AgentIdentity;
use crate::{agent_control::agent_id::AgentID, sub_agent::error::SubAgentError};
use opamp_client::{
    StartedClient,
    operation::settings::{AgentDescription, DescriptionValueType, StartSettings},
};
use std::collections::HashMap;
use tracing::info;

pub fn sub_agent_start_settings<IG: InstanceIDGetter>(
    instance_id_getter: &IG,
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    mut non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<StartSettings, OpAMPClientBuilderError> {
    let agent_control_id = AgentID::AgentControl;
    let parent_instance_id = instance_id_getter.get(&agent_control_id)?;

    non_identifying_attributes.insert(
        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::Bytes(parent_instance_id.into()),
    );

    Ok(StartSettings {
        instance_uid: instance_id_getter.get(&agent_identity.id)?.into(),
        capabilities: default_capabilities(),
        custom_capabilities: Some(default_custom_capabilities()),
        agent_description: agent_description(
            agent_identity,
            additional_identifying_attributes,
            non_identifying_attributes,
        ),
    })
}

/// Builds [AgentDescription] from the provided [AgentIdentity] and additional attributes
pub fn agent_description(
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> AgentDescription {
    let mut identifying_attributes = HashMap::from([
        (
            OPAMP_SERVICE_NAME.to_string(),
            agent_identity.agent_type_id.name().into(),
        ),
        (
            OPAMP_SERVICE_NAMESPACE.to_string(),
            agent_identity.agent_type_id.namespace().into(),
        ),
        (
            OPAMP_SUPERVISOR_KEY.to_string(),
            agent_identity.id.to_string().into(),
        ),
    ]);

    identifying_attributes.extend(additional_identifying_attributes);

    AgentDescription {
        identifying_attributes,
        non_identifying_attributes,
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
        client.stop()?;
    }
    Ok(())
}
