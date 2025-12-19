use crate::checkers::status::AgentStatus;
use crate::sub_agent::error::SubAgentError;
use opamp_client::StartedClient;
use opamp_client::opamp::proto::{AnyValue, KeyValue, any_value};

/// This method request the AgentDescription from the current opamp client and, updates or add the
/// field from agent version to be sent to opamp server
pub fn set_agent_description_status<C>(
    opamp_client: &C,
    status: AgentStatus,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    let mut agent_description = opamp_client.get_agent_description()?;
    agent_description.identifying_attributes =
        update_status_key_values(agent_description.identifying_attributes, status);
    Ok(opamp_client.set_agent_description(agent_description)?)
}

fn update_status_key_values(
    mut key_values: Vec<KeyValue>,
    agent_data: AgentStatus,
) -> Vec<KeyValue> {
    let status_value = Some(AnyValue {
        value: Some(any_value::Value::StringValue(agent_data.status)),
    });
    if let Some(attribute) = key_values
        .iter_mut()
        .find(|attr| attr.key == agent_data.opamp_field)
    {
        attribute.value = status_value;
    } else {
        key_values.push(KeyValue {
            key: agent_data.opamp_field,
            value: status_value,
        });
    }
    key_values
}
