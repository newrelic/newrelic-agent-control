use crate::common::opamp::FakeServer;
use newrelic_agent_control::opamp::instance_id::InstanceID;

use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AnyValue, KeyValue};

pub fn check_latest_identifying_attributes_match_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_identifying_attributes: Vec<KeyValue>,
) -> Result<(), String> {
    let current_attributes = opamp_server
        .get_attributes(instance_id)
        .ok_or_else(|| "Identifying attributes not found".to_string())?;

    check_opamp_attributes(
        expected_identifying_attributes.clone(),
        current_attributes.identifying_attributes.clone(),
    )
    .map_err(|e| format!("Identifying attributes don't match {e}:"))
}

pub fn check_latest_non_identifying_attributes_match_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_non_identifying_attributes: Vec<KeyValue>,
) -> Result<(), String> {
    let current_attributes = opamp_server
        .get_attributes(instance_id)
        .ok_or_else(|| "Non identifying attributes not found".to_string())?;

    check_opamp_attributes(
        expected_non_identifying_attributes.clone(),
        current_attributes.non_identifying_attributes.clone(),
    )
    .map_err(|e| format!("Non identifying attributes don't match: {e}"))
}

pub fn check_identifying_attributes_contains_expected(
    opamp_server: &FakeServer,
    instance_id: &InstanceID,
    expected_subset: Vec<KeyValue>,
) -> Result<(), String> {
    let current_attributes = opamp_server
        .get_attributes(instance_id)
        .ok_or_else(|| "Identifying attributes not found".to_string())?;

    check_opamp_attributes_contains(
        expected_subset,
        current_attributes.identifying_attributes.clone(),
    )
    .map_err(|e| format!("Identifying attributes missing required elements: {e}"))
}

fn check_opamp_attributes(
    mut expected_vec: Vec<KeyValue>,
    mut current_vec: Vec<KeyValue>,
) -> Result<(), String> {
    expected_vec.sort_by(|a, b| a.key.cmp(&b.key));
    current_vec.sort_by(|a, b| a.key.cmp(&b.key));
    if expected_vec != current_vec {
        return Err(format!(
            "Expected != Found\nExpected:\n{expected_vec:?}\nFound:\n{current_vec:?}\n"
        ));
    }
    Ok(())
}

fn check_opamp_attributes_contains(
    subset_vec: Vec<KeyValue>,
    superset_vec: Vec<KeyValue>,
) -> Result<(), String> {
    for expected in subset_vec {
        let found = superset_vec
            .iter()
            .find(|&current| current.key == expected.key && current.value == expected.value);

        if found.is_none() {
            return Err(format!(
                "Required attribute key '{}' with value '{:?}' not found in actual attributes.",
                expected.key, expected.value
            ));
        }
    }
    Ok(())
}

pub fn convert_to_vec_key_value(data: Vec<(&str, Value)>) -> Vec<KeyValue> {
    data.into_iter()
        .map(|(k, v)| KeyValue {
            key: k.to_string(),
            value: Some(AnyValue { value: Some(v) }),
        })
        .collect()
}
