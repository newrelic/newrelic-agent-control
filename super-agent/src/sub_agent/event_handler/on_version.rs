use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::version::version_checker::AgentVersion;
use opamp_client::opamp::proto::{any_value, AgentDescription, AnyValue, KeyValue};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;

/// This method request the AgentDescription from the current opamp client and, updates or add the
/// field from agent version to be sent to opamp server
pub fn on_version<C, CB>(
    agent_data: AgentVersion,
    maybe_opamp_client: Option<&C>,
) -> Result<(), SubAgentError>
where
    C: StartedClient<CB>,
    CB: Callbacks,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        let agent_description = client.get_agent_description()?;
        client.set_agent_description(add_or_change_chart_version_into_agent_description(
            agent_description,
            agent_data,
        ))?;
    }
    Ok(())
}

fn add_or_change_chart_version_into_agent_description(
    mut agent_description: AgentDescription,
    agent_data: AgentVersion,
) -> AgentDescription {
    let version_info = Some(AnyValue {
        value: Some(any_value::Value::StringValue(
            agent_data.version().to_string(),
        )),
    });
    if let Some(attribute) = agent_description
        .identifying_attributes
        .iter_mut()
        .find(|attr| attr.key == agent_data.opamp_field())
    {
        attribute.value = version_info;
    } else {
        agent_description.identifying_attributes.push(KeyValue {
            key: agent_data.opamp_field().to_string(),
            value: version_info,
        });
    }
    agent_description
}
