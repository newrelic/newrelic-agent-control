use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::version::version_checker::AgentVersion;
use opamp_client::opamp::proto::{any_value, AgentDescription, AnyValue, KeyValue};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::{client, StartedClient};
use crate::super_agent::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;

pub fn on_version<C, CB>(
    version: AgentVersion,
    maybe_opamp_client: Option<&C>,
) -> Result<(), SubAgentError>
where
    C: StartedClient<CB>,
    CB: Callbacks,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        let agent_description = client.get_agent_description();
        client.set_agent_description(add_or_change_chart_version_into_agent_description(
            agent_description?,
            version.version().to_string(),
        ))?;
    }
    Ok(())
}

fn add_or_change_chart_version_into_agent_description(
    mut agent_description: AgentDescription,
    version: String,
) -> AgentDescription {
    if let Some(attribute) = agent_description
        .identifying_attributes
        .iter_mut()
        .find(|attr| attr.key == OPAMP_CHART_VERSION_ATTRIBUTE_KEY)
    {
        attribute.value = Some(AnyValue {
            value: Some(any_value::Value::StringValue(version)),
        });
    } else {
        agent_description.identifying_attributes.push(KeyValue {
            key: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(version)),
            }),
        });
    }

    agent_description
}
