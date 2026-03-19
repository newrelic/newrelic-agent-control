use super::instance_id::InstanceID;
use super::{
    client_builder::{BuildOpAMPClient, OpAMPClientBuilderError},
    instance_id::getter::InstanceIDGetter,
};
use crate::agent_control::defaults::{
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
    PARENT_AGENT_ID_ATTRIBUTE_KEY, default_capabilities, default_custom_capabilities,
};
use crate::sub_agent::identity::AgentIdentity;
use crate::{
    agent_control::agent_id::AgentID,
    event::{OpAMPEvent, channel::EventConsumer},
    sub_agent::error::SubAgentError,
};
use opamp_client::{
    StartedClient,
    operation::settings::{AgentDescription, DescriptionValueType, StartSettings},
};
use semver::Version;
use std::collections::HashMap;
use tracing::info;

/// Type alias for an OpAMP client and its event consumer.
pub type OpAMPClientAndConsumer<C> = (C, EventConsumer<OpAMPEvent>);

/// Builds and starts an OpAMP client for Agent Control if the builder is not None.
pub fn maybe_build_agent_control_opamp<OB, IG>(
    opamp_builder: Option<&OB>,
    instance_id_getter: &IG,
    identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<Option<OpAMPClientAndConsumer<OB::Client>>, OpAMPClientBuilderError>
where
    OB: BuildOpAMPClient,
    IG: InstanceIDGetter,
{
    let Some(opamp_builder) = opamp_builder else {
        return Ok(None);
    };

    info!("Building and starting OpAMP client for the agent control",);
    let agent_identity = AgentIdentity::new_agent_control_identity();

    let start_settings = start_settings(
        instance_id_getter.get(&agent_identity.id)?,
        &agent_identity,
        identifying_attributes,
        non_identifying_attributes,
    );

    opamp_builder
        .build_and_start(agent_identity, start_settings)
        .map(Some)
}

/// Builds and starts an OpAMP client for a sub-agent if the builder is not None.
/// Automatically adds the parent agent ID to non-identifying attributes.
pub fn maybe_build_sub_agent_opamp<OB, IG>(
    opamp_builder: Option<&OB>,
    instance_id_getter: &IG,
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    mut non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> Result<Option<OpAMPClientAndConsumer<OB::Client>>, OpAMPClientBuilderError>
where
    OB: BuildOpAMPClient,
    IG: InstanceIDGetter,
{
    let Some(opamp_builder) = opamp_builder else {
        return Ok(None);
    };

    // Add parent agent ID to non-identifying attributes
    non_identifying_attributes.insert(
        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::Bytes(instance_id_getter.get(&AgentID::AgentControl)?.into()),
    );

    info!(
        "Building and starting OpAMP client for {}",
        agent_identity.id
    );

    let start_settings = start_settings(
        instance_id_getter.get(&agent_identity.id)?,
        agent_identity,
        additional_identifying_attributes,
        non_identifying_attributes,
    );

    opamp_builder
        .build_and_start(agent_identity.clone(), start_settings)
        .map(Some)
}

pub fn agent_control_service_version_attribute(
    version: &Version,
) -> HashMap<String, DescriptionValueType> {
    HashMap::from([(
        OPAMP_SERVICE_VERSION.to_string(),
        DescriptionValueType::String(version.to_string()),
    )])
}

/// Builds the OpAMP StartSettings corresponding to the provided arguments for any sub agent and agent control.
pub fn start_settings(
    instance_id: InstanceID,
    agent_identity: &AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
) -> StartSettings {
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

    StartSettings {
        instance_uid: instance_id.into(),
        capabilities: default_capabilities(),
        custom_capabilities: Some(default_custom_capabilities()),
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
        client.stop()?;
    }
    Ok(())
}
