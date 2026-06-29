//! Helpers to update and publish OpAMP agent attributes (the agent description).
use std::fmt::Debug;

use opamp_client::opamp::proto::{AgentDescription as ProtoAgentDescription, KeyValue};
use opamp_client::operation::settings::AgentDescription;
use opamp_client::{ClientError, StartedClient};
use tracing::error;

use crate::event::channel::EventPublisher;

/// Event message type for updating OpAMP agent attributes
pub type UpdatedAttributesMessage = AgentDescription;

/// Updates the attributes of the OpAMP agent
///
/// The provided `agent_description` is expected to contain the attributes to be updated only.
/// If an attribute already exists, it will be updated. If it doesn't, it will be added. No
/// attribute is removed in any case.
pub fn update_opamp_attributes<C>(
    opamp_client: &C,
    agent_description: AgentDescription,
) -> Result<(), ClientError>
where
    C: StartedClient,
{
    let current_agent_description = opamp_client.get_agent_description()?;
    let updated_agent_description =
        merge_agent_description(current_agent_description, agent_description);

    opamp_client.set_agent_description(updated_agent_description)
}

fn merge_agent_description(
    old_agent_description: ProtoAgentDescription,
    new_attributes: AgentDescription,
) -> ProtoAgentDescription {
    let new_proto: ProtoAgentDescription = new_attributes.into();
    let mut agent_description = old_agent_description;
    agent_description.identifying_attributes = merge_attributes(
        agent_description.identifying_attributes,
        new_proto.identifying_attributes.into_iter(),
    );
    agent_description.non_identifying_attributes = merge_attributes(
        agent_description.non_identifying_attributes,
        new_proto.non_identifying_attributes.into_iter(),
    );
    agent_description
}

/// Merges new attributes into old attributes
///
/// If an attribute already exists, it will be updated. If it doesn't, it will be added.
fn merge_attributes(
    old_attributes: Vec<KeyValue>,
    new_attributes: impl Iterator<Item = KeyValue>,
) -> Vec<KeyValue> {
    let mut merged_attributes = old_attributes;
    for new_kv in new_attributes {
        match merged_attributes.iter().position(|kv| kv.key == new_kv.key) {
            Some(index) => merged_attributes[index].value = new_kv.value,
            None => merged_attributes.push(new_kv),
        }
    }

    merged_attributes
}

/// Publishes an attribute-update event, logging an error if publishing fails.
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
    use opamp_client::opamp::proto::any_value::Value;
    use opamp_client::opamp::proto::{AnyValue, KeyValue};
    use opamp_client::operation::settings::DescriptionValueType;
    use std::collections::HashMap;

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
        let agent_description = ProtoAgentDescription {
            identifying_attributes: vec![
                new_key_value("identifying1", "value1"),
                new_key_value("identifying3", "value3"),
                new_key_value("identifying5", "value5"),
            ],
            non_identifying_attributes: vec![new_key_value("non_identifying1", "value1")],
        };

        let updated_description = merge_agent_description(
            agent_description.clone(),
            AgentDescription {
                identifying_attributes: HashMap::from([
                    (
                        "identifying2".to_string(),
                        DescriptionValueType::String("new_value2".to_string()),
                    ),
                    (
                        "identifying3".to_string(),
                        DescriptionValueType::String("new_value3".to_string()),
                    ),
                    (
                        "identifying4".to_string(),
                        DescriptionValueType::String("new_value4".to_string()),
                    ),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "non_identifying2".to_string(),
                    DescriptionValueType::String("new_value2".to_string()),
                )]),
            },
        );

        // Sort both sides before comparing since HashMap iteration order is non-deterministic.
        let mut actual_identifying = updated_description.identifying_attributes;
        actual_identifying.sort_by_key(|kv| kv.key.clone());
        let mut expected_identifying = vec![
            new_key_value("identifying1", "value1"),
            new_key_value("identifying2", "new_value2"),
            new_key_value("identifying3", "new_value3"),
            new_key_value("identifying4", "new_value4"),
            new_key_value("identifying5", "value5"),
        ];
        expected_identifying.sort_by_key(|kv| kv.key.clone());
        assert_eq!(expected_identifying, actual_identifying);

        let mut actual_non_identifying = updated_description.non_identifying_attributes;
        actual_non_identifying.sort_by_key(|kv| kv.key.clone());
        let mut expected_non_identifying = vec![
            new_key_value("non_identifying1", "value1"),
            new_key_value("non_identifying2", "new_value2"),
        ];
        expected_non_identifying.sort_by_key(|kv| kv.key.clone());
        assert_eq!(expected_non_identifying, actual_non_identifying);
    }
}
