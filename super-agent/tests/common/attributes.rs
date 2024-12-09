use crate::common::opamp::FakeServer;
use newrelic_super_agent::opamp::instance_id::InstanceID;
use newrelic_super_agent::super_agent::defaults::{
    HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_CHART_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION,
    PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use nix::unistd::gethostname;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
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
    .map_err(|e| format!("Identifying {}", e))
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
    .map_err(|e| format!("Non identifying {}", e))
}
fn check_opamp_attributes(
    mut expected_vec: Vec<KeyValue>,
    mut current_vec: Vec<KeyValue>,
) -> Result<(), String> {
    expected_vec.sort_by(|a, b| a.key.cmp(&b.key));
    current_vec.sort_by(|a, b| a.key.cmp(&b.key));
    println!("Expected: {:?}", expected_vec);
    println!("Current: {:?}", current_vec);
    if expected_vec != current_vec {
        return Err(format!(
            "not as expected, Expected: {:?}, Found: {:?}",
            expected_vec, current_vec
        ));
    }
    Ok(())
}
pub fn get_expected_identifying_attributes(
    namespace: String,
    service_name: String,
    service_version: Option<String>,
    agent_version: Option<String>,
    chart_version: Option<String>,
) -> Vec<KeyValue> {
    let mut y: Vec<KeyValue> = Vec::from([
        (KeyValue {
            key: OPAMP_SERVICE_NAMESPACE.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(namespace)),
            }),
        }),
        (KeyValue {
            key: OPAMP_SERVICE_NAME.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(service_name)),
            }),
        }),
    ]);
    if let Some(service_version) = service_version {
        y.push(KeyValue {
            key: OPAMP_SERVICE_VERSION.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(service_version)),
            }),
        })
    }
    if let Some(agent_version) = agent_version {
        y.push(KeyValue {
            key: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(agent_version)),
            }),
        })
    }
    if let Some(chart_version) = chart_version {
        y.push(KeyValue {
            key: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(chart_version)),
            }),
        })
    }
    y
}
pub fn get_expected_non_identifying_attributes(instace_id: InstanceID) -> Vec<KeyValue> {
    Vec::from([
        (KeyValue {
            key: HOST_NAME_ATTRIBUTE_KEY.to_string(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(
                    gethostname().unwrap_or_default().into_string().unwrap(),
                )),
            }),
        }),
        (KeyValue {
            key: PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
            value: Some(AnyValue {
                value: Some(BytesValue(instace_id.into())),
            }),
        }),
    ])
}
