use super::instance_id::InstanceID;
use super::{
    client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    instance_id::getter::InstanceIDGetter,
};
use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::defaults::{
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SUPERVISOR_KEY,
    PARENT_AGENT_ID_ATTRIBUTE_KEY, default_capabilities, default_custom_capabilities,
};
use crate::agent_control::run::RunError;
use crate::http::config::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::{HttpClientBuilder, OpAMPHttpClientBuilder};
use crate::secret_retriever::OpampSecretRetriever;
use crate::sub_agent::identity::AgentIdentity;
use crate::{
    agent_control::agent_id::AgentID,
    event::{
        OpAMPEvent,
        channel::{EventConsumer, pub_sub},
    },
    sub_agent::error::SubAgentError,
};
use opamp_client::{
    StartedClient,
    operation::settings::{AgentDescription, DescriptionValueType, StartSettings},
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

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
    let agent_control_id = AgentID::AgentControl;
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
        agent_identity,
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

pub fn build_opamp_http_builder<R>(
    opamp_config: Option<OpAMPClientConfig>,
    proxy: ProxyConfig,
    retriever: R,
) -> Result<Option<impl HttpClientBuilder>, RunError>
where
    R: OpampSecretRetriever,
{
    if let Some(opamp_config) = opamp_config {
        debug!("OpAMP configuration found, creating an OpAMP client builder");

        let private_key = retriever
            .retrieve()
            .map_err(|e| RunError(format!("error trying to get secret or private key {e}")))?;

        let token_retriever = Arc::new(
            TokenRetrieverImpl::try_build(
                opamp_config.clone().auth_config,
                private_key,
                proxy.clone(),
            )
            .inspect_err(|err| error!("Could not build OpAMP's token retriever: {err}"))
            .map_err(|e| {
                RunError(format!(
                    "error trying to build OpAMP's token retriever: {e}"
                ))
            })?,
        );

        let http_builder = OpAMPHttpClientBuilder::new(opamp_config, proxy, token_retriever);

        Ok(Some(http_builder))
    } else {
        Ok(None)
    }
}
