use std::error::Error;
use std::fmt::Debug;

use opamp_client::StartedClient;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AgentDescription, AnyValue, KeyValue};
use tracing::error;

use crate::event::channel::EventPublisher;

/// Event message type for updating OpAMP agent attributes
pub type UpdateAttributesMessage = Vec<Attribute>;

/// Represents the type of an [AgentDescription message in OpAMP](https://opentelemetry.io/docs/specs/opamp/#agentdescription-message)
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeType {
    Identifying,
    NonIdentifying,
}

/// Represents an agent attribute
///
/// Simple wrapper for [`KeyValue`] and it's type, that simplifies the creation
/// of agent attributes.
///
/// ## Example:
/// ```
/// # use newrelic_agent_control::opamp::attributes::{Attribute, AttributeType};
/// let attr = Attribute::from((AttributeType::Identifying, "key", "value"));
/// ```
/// instead of (assume we don't care about the type for this example):
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
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    attribute_type: AttributeType,
    key_value: KeyValue,
}

impl Attribute {
    /// Returns the inner [`KeyValue`]
    pub fn key_value(self) -> KeyValue {
        self.key_value
    }
}

impl<K, V> From<(AttributeType, K, V)> for Attribute
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    fn from((attribute_type, key, value): (AttributeType, K, V)) -> Self {
        Attribute {
            attribute_type,
            key_value: KeyValue {
                key: String::from(key.as_ref()),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(String::from(value.as_ref()))),
                }),
            },
        }
    }
}

/// Updates the attributes of the OpAMP agent
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicated keys, the last occurrence will be kept.
pub fn update_opamp_attributes<C>(
    opamp_client: &C,
    new_attributes: Vec<Attribute>,
) -> Result<(), Box<dyn Error>>
where
    C: StartedClient,
{
    let agent_description = opamp_client.get_agent_description()?;
    let updated_agent_description =
        update_agent_description_attributes(agent_description, new_attributes);

    Ok(opamp_client.set_agent_description(updated_agent_description)?)
}

fn update_agent_description_attributes(
    mut agent_description: AgentDescription,
    new_attributes: Vec<Attribute>,
) -> AgentDescription {
    let (new_identifying_attributes, new_non_identifying_attributes): (Vec<_>, Vec<_>) =
        new_attributes
            .into_iter()
            .partition(|attribute| attribute.attribute_type == AttributeType::Identifying);

    let key_value_iter = |a: Vec<Attribute>| a.into_iter().map(Attribute::key_value);
    agent_description.identifying_attributes = merge_attributes(
        agent_description.identifying_attributes,
        key_value_iter(new_identifying_attributes),
    );

    agent_description.non_identifying_attributes = merge_attributes(
        agent_description.non_identifying_attributes,
        key_value_iter(new_non_identifying_attributes),
    );

    agent_description
}

/// Merges new attributes into old attributes
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
/// If the `new_attributes` contain duplicated keys, the last occurrence will be kept.
fn merge_attributes(
    mut old_attributes: Vec<KeyValue>,
    new_attributes: impl Iterator<Item = KeyValue>,
) -> Vec<KeyValue> {
    for new_kv in new_attributes {
        match old_attributes.iter().position(|kv| kv.key == new_kv.key) {
            Some(index) => old_attributes[index].value = new_kv.value,
            None => old_attributes.push(new_kv),
        }
    }

    old_attributes
}

pub fn publish_update_attributes_event<T>(event_publisher: &EventPublisher<T>, event: T)
where
    T: Debug + Send + Sync + 'static,
{
    let event_type_str = format!("{event:?}");
    _ = event_publisher.publish(event).inspect_err(|e| {
        error!(
            err = e.to_string(),
            event_type = event_type_str,
            "could not update attributes event"
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_key_value(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: String::from(key),
            value: Some(AnyValue {
                value: Some(Value::StringValue(String::from(value))),
            }),
        }
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
        };

        // Check identifying attributes are correctly updated
        // Here we check behaviour like order and duplicates
        let updated_description = update_agent_description_attributes(
            agent_description.clone(),
            vec![
                Attribute::from((AttributeType::Identifying, "identifying2", "new_value2")),
                Attribute::from((AttributeType::Identifying, "identifying3", "new_value3")),
                Attribute::from((
                    AttributeType::NonIdentifying,
                    "non_identifying2",
                    "new_value2",
                )),
                Attribute::from((AttributeType::Identifying, "identifying4", "new_value4")),
                Attribute::from((
                    AttributeType::Identifying,
                    "identifying4",
                    "duplicated_value4",
                )),
            ],
        );

        let expected_identifying_attributes = vec![
            new_key_value("identifying1", "value1"),
            new_key_value("identifying3", "new_value3"),
            new_key_value("identifying5", "value5"),
            new_key_value("identifying2", "new_value2"),
            new_key_value("identifying4", "duplicated_value4"),
        ];
        assert_eq!(
            expected_identifying_attributes,
            updated_description.identifying_attributes
        );

        let expected_non_identifying_attributes = vec![
            new_key_value("non_identifying1", "value1"),
            new_key_value("non_identifying2", "new_value2"),
        ];
        assert_eq!(
            expected_non_identifying_attributes,
            updated_description.non_identifying_attributes
        );
    }
}
