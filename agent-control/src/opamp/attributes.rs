use crate::sub_agent::error::SubAgentError;
use opamp_client::StartedClient;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AgentDescription, AnyValue, KeyValue};

/// Represents an agent attribute
///
/// Simple wrapper for [`KeyValue`] that simplifies the creation
/// of agent attributes.
///
/// ## Example:
/// ```
/// # use newrelic_agent_control::opamp::attributes::Attribute;
/// let attr = Attribute::from(("key", "value"));
/// ```
/// instead of:
/// ```
/// # use opamp_client::opamp::proto::any_value::Value;
/// # use opamp_client::opamp::proto::{AnyValue, KeyValue};
/// let attr = KeyValue {
///     key: "key".into(),
///     value: Some(AnyValue {
///         value: Some(Value::StringValue("value".into())),
///     }),
/// };
/// ```
pub struct Attribute(KeyValue);

impl<K, V> From<(K, V)> for Attribute
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    fn from((key, value): (K, V)) -> Self {
        Attribute(KeyValue {
            key: String::from(key.as_ref()),
            value: Some(AnyValue {
                value: Some(Value::StringValue(String::from(value.as_ref()))),
            }),
        })
    }
}

/// Updates the identifying attributes of the OpAMP agent
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicates, the last occurrence will be kept.
pub fn update_identifying_attributes<C>(
    opamp_client: &C,
    new_attributes: Vec<Attribute>,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    let agent_description = opamp_client.get_agent_description()?;
    let updated_agent_description =
        update_agent_description_attributes(agent_description, true, new_attributes);

    Ok(opamp_client.set_agent_description(updated_agent_description)?)
}

/// Updates the non-identifying attributes of the OpAMP agent
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicates, the last occurrence will be kept.
pub fn update_non_identifying_attributes<C>(
    opamp_client: &C,
    new_attributes: Vec<Attribute>,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    let agent_description = opamp_client.get_agent_description()?;
    let updated_agent_description =
        update_agent_description_attributes(agent_description, false, new_attributes);

    Ok(opamp_client.set_agent_description(updated_agent_description)?)
}

fn update_agent_description_attributes(
    agent_description: AgentDescription,
    identifying_attributes: bool,
    new_attributes: Vec<Attribute>,
) -> AgentDescription {
    let new_attributes: Vec<KeyValue> = new_attributes.into_iter().map(|attr| attr.0).collect();

    let mut updated_description = agent_description;
    let attributes = if identifying_attributes {
        &mut updated_description.identifying_attributes
    } else {
        &mut updated_description.non_identifying_attributes
    };

    *attributes = merge_attributes(std::mem::take(attributes), new_attributes);

    updated_description
}

/// Merges new attributes into old attributes
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicates, the last occurrence will be kept.
fn merge_attributes(
    mut old_attributes: Vec<KeyValue>,
    new_attributes: Vec<KeyValue>,
) -> Vec<KeyValue> {
    for new_kv in new_attributes {
        match old_attributes.iter().position(|kv| kv.key == new_kv.key) {
            Some(index) => old_attributes[index].value = new_kv.value,
            None => old_attributes.push(new_kv),
        }
    }

    old_attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_key_value(key: &str, value: &str) -> KeyValue {
        Attribute::from((key, value)).0
    }

    #[test]
    fn test_update_agent_attributes() {
        // Create base agent description
        let agent_description = AgentDescription {
            identifying_attributes: vec![
                new_key_value("identifying1", "value1"),
                new_key_value("identifying3", "value3"),
                new_key_value("identifying5", "value5"),
            ],
            non_identifying_attributes: vec![new_key_value("non_identifying1", "value1")],
            ..Default::default()
        };

        // Check identifying attributes are correctly updated
        // Here we check behaviour like order and duplicates
        let updated_description = update_agent_description_attributes(
            agent_description.clone(),
            true,
            vec![
                Attribute::from(("identifying2", "new_value2")),
                Attribute::from(("identifying3", "new_value3")),
                Attribute::from(("identifying4", "new_value4")),
                Attribute::from(("identifying4", "duplicated_value4")),
            ],
        );

        let expected_attributes = vec![
            new_key_value("identifying1", "value1"),
            new_key_value("identifying3", "new_value3"),
            new_key_value("identifying5", "value5"),
            new_key_value("identifying2", "new_value2"),
            new_key_value("identifying4", "duplicated_value4"),
        ];
        assert_eq!(
            expected_attributes,
            updated_description.identifying_attributes
        );

        // Check non-identifying attributes are correctly updated
        // We already checked behaviour, so we focus on a simple case here
        let updated_description = update_agent_description_attributes(
            agent_description,
            false,
            vec![Attribute::from(("non_identifying2", "new_value2"))],
        );

        let expected_attributes = vec![
            new_key_value("non_identifying1", "value1"),
            new_key_value("non_identifying2", "new_value2"),
        ];
        assert_eq!(
            expected_attributes,
            updated_description.non_identifying_attributes
        );
    }
}
