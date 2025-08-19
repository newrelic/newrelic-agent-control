use crate::sub_agent::error::SubAgentError;
use crate::version_checker::AgentVersion;
use opamp_client::StartedClient;
use opamp_client::opamp::proto::{AnyValue, KeyValue, any_value};

/// This method request the AgentDescription from the current opamp client and, updates or add the
/// field from agent version to be sent to opamp server
pub fn on_version<C>(
    agent_data: AgentVersion,
    maybe_opamp_client: Option<&C>,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        let mut agent_description = client.get_agent_description()?;
        agent_description.identifying_attributes =
            update_version_key_values(agent_description.identifying_attributes, agent_data);
        client.set_agent_description(agent_description)?;
    }
    Ok(())
}

fn update_version_key_values(
    mut key_values: Vec<KeyValue>,
    agent_data: AgentVersion,
) -> Vec<KeyValue> {
    let version_value = Some(AnyValue {
        value: Some(any_value::Value::StringValue(agent_data.version)),
    });
    if let Some(attribute) = key_values
        .iter_mut()
        .find(|attr| attr.key == agent_data.opamp_field)
    {
        attribute.value = version_value;
    } else {
        key_values.push(KeyValue {
            key: agent_data.opamp_field,
            value: version_value,
        });
    }
    key_values
}
