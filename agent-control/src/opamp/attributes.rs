use crate::sub_agent::error::SubAgentError;
use opamp_client::StartedClient;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AnyValue, KeyValue};

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
    let new_attributes: Vec<KeyValue> = new_attributes.into_iter().map(|attr| attr.0).collect();

    let mut agent_description = opamp_client.get_agent_description()?;
    agent_description.identifying_attributes =
        update_attributes(agent_description.identifying_attributes, new_attributes);

    Ok(opamp_client.set_agent_description(agent_description)?)
}

/// Updates a list of attributes
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicates, the last occurrence will be kept.
fn update_attributes(
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

    #[test]
    fn test_update_key_values() {
        let new_key_value = |key: &str, value: &str| Attribute::from((key, value)).0;

        let old_attributes = vec![
            new_key_value("key1", "value1"),
            new_key_value("key3", "value3"),
            new_key_value("key5", "value5"),
        ];
        let new_attributes = vec![
            new_key_value("key2", "new_value2"),
            new_key_value("key3", "new_value3"),
            new_key_value("key4", "new_value4"),
            new_key_value("key4", "duplicated_value4"),
        ];
        let updated_attributes = update_attributes(old_attributes, new_attributes);

        let expected_attributes = vec![
            new_key_value("key1", "value1"),
            new_key_value("key3", "new_value3"),
            new_key_value("key5", "value5"),
            new_key_value("key2", "new_value2"),
            new_key_value("key4", "duplicated_value4"),
        ];
        assert_eq!(expected_attributes, updated_attributes);
    }
}
