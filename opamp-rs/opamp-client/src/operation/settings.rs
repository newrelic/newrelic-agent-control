//! Common parameters for starting an OpAMP client for a specific agent.

use std::collections::HashMap;

use crate::opamp::proto::{
    any_value::Value, AgentDescription as ProtobufAgentDescription, AnyValue, KeyValue,
};

use super::capabilities::Capabilities;

#[derive(Debug, PartialEq, Clone, Default)]
/// Internal representation of the OpAMP AgentDescription: https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agentdescription-message
pub struct AgentDescription {
    /// Attributes that identify the Agent. See: https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agentdescriptionidentifying_attributes
    pub identifying_attributes: HashMap<String, DescriptionValueType>,

    /// Attributes that do not necessarily identify the Agent but help describe where it runs. See: https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agentdescriptionnon_identifying_attributes
    pub non_identifying_attributes: HashMap<String, DescriptionValueType>,
}

#[derive(Debug, PartialEq, Clone)]
/// Values type mapping for the AgentDescription attributes
pub enum DescriptionValueType {
    /// String wrapper
    String(String),
    /// i64 wrapper
    Int(i64),
    /// bool wrapper
    Bool(bool),
    /// f64 wrapper
    Float(f64),
    // Array(Vec<DescType>),
    // Map(HashMap<String, DescType>),
    /// Vec<u8> wrapper
    Bytes(Vec<u8>),
}

impl From<DescriptionValueType> for Option<AnyValue> {
    fn from(description: DescriptionValueType) -> Self {
        match description {
            DescriptionValueType::String(s) => Some(AnyValue {
                value: Some(Value::StringValue(s)),
            }),
            DescriptionValueType::Int(i) => Some(AnyValue {
                value: Some(Value::IntValue(i)),
            }),
            DescriptionValueType::Bool(b) => Some(AnyValue {
                value: Some(Value::BoolValue(b)),
            }),
            DescriptionValueType::Float(f) => Some(AnyValue {
                value: Some(Value::DoubleValue(f)),
            }),
            DescriptionValueType::Bytes(b) => Some(AnyValue {
                value: Some(Value::BytesValue(b)),
            }),
        }
    }
}

impl From<&str> for DescriptionValueType {
    fn from(s: &str) -> Self {
        DescriptionValueType::String(s.to_string())
    }
}

impl From<String> for DescriptionValueType {
    fn from(s: String) -> Self {
        DescriptionValueType::String(s)
    }
}

impl From<i64> for DescriptionValueType {
    fn from(i: i64) -> Self {
        DescriptionValueType::Int(i)
    }
}

impl From<bool> for DescriptionValueType {
    fn from(b: bool) -> Self {
        DescriptionValueType::Bool(b)
    }
}

impl From<f64> for DescriptionValueType {
    fn from(f: f64) -> Self {
        DescriptionValueType::Float(f)
    }
}

impl From<Vec<u8>> for DescriptionValueType {
    fn from(b: Vec<u8>) -> Self {
        DescriptionValueType::Bytes(b)
    }
}

impl From<AgentDescription> for ProtobufAgentDescription {
    fn from(agent_description: AgentDescription) -> Self {
        ProtobufAgentDescription {
            identifying_attributes: populate_agent_description(
                agent_description.identifying_attributes,
            ),
            non_identifying_attributes: populate_agent_description(
                agent_description.non_identifying_attributes,
            ),
        }
    }
}

fn populate_agent_description(attrs: HashMap<String, DescriptionValueType>) -> Vec<KeyValue> {
    let mut result = Vec::new();
    for (key, desc_value) in attrs {
        let key_value = KeyValue {
            key,
            value: desc_value.into(),
        };
        result.push(key_value);
    }
    result
}

/// StartSettings defines the parameters for starting the OpAMP Client.
#[derive(Debug, PartialEq, Default)]
pub struct StartSettings {
    /// Agent information.
    pub instance_id: Vec<u8>,

    /// Defines the capabilities of the Agent. AgentCapabilities_ReportsStatus bit does not need to
    /// be set in this field, it will be set automatically since it is required by OpAMP protocol.
    pub capabilities: Capabilities,

    /// Agent's description: https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agentdescription-message
    pub agent_description: AgentDescription,
}

#[cfg(test)]
mod test {
    use crate::operation::settings::{
        populate_agent_description, AgentDescription, DescriptionValueType,
    };
    use std::collections::HashMap;

    use crate::opamp::proto::any_value::Value::{
        BoolValue, BytesValue, DoubleValue, IntValue, StringValue,
    };
    use crate::opamp::proto::{
        any_value::Value, AgentDescription as ProtobufAgentDescription, AnyValue, KeyValue,
    };

    #[test]
    fn agent_description_supports_multiple_types() {
        let bytes: Vec<u8> = vec![
            1, 143, 243, 141, 1, 179, 119, 150, 178, 200, 28, 128, 105, 188, 106, 223,
        ];

        let agent_description = AgentDescription {
            identifying_attributes: HashMap::from([
                (
                    "string".to_string(),
                    DescriptionValueType::String("some string".to_string()),
                ),
                ("int".to_string(), DescriptionValueType::Int(45)),
                ("bool".to_string(), DescriptionValueType::Bool(true)),
                ("float".to_string(), DescriptionValueType::Float(5.6)),
                (
                    "bytes".to_string(),
                    DescriptionValueType::Bytes(bytes.clone()),
                ),
            ]),
            non_identifying_attributes: HashMap::from([
                ("string".into(), "another string".into()),
                ("another int".to_string(), 145.into()),
                ("another bool".to_string(), false.into()),
                ("another float".to_string(), 15.6.into()),
                (
                    "bytes".to_string(),
                    DescriptionValueType::Bytes(bytes.clone()),
                ),
            ]),
        };

        assert_eq!(
            agent_description
                .identifying_attributes
                .get("string")
                .unwrap(),
            &DescriptionValueType::String("some string".to_string())
        );

        assert_eq!(
            agent_description
                .non_identifying_attributes
                .get("string")
                .unwrap(),
            &DescriptionValueType::String("another string".to_string())
        );

        assert_eq!(
            agent_description.identifying_attributes.get("int").unwrap(),
            &DescriptionValueType::Int(45)
        );

        assert_eq!(
            agent_description
                .non_identifying_attributes
                .get("another int")
                .unwrap(),
            &DescriptionValueType::Int(145)
        );

        assert_eq!(
            agent_description
                .identifying_attributes
                .get("bool")
                .unwrap(),
            &DescriptionValueType::Bool(true)
        );

        assert_eq!(
            agent_description
                .identifying_attributes
                .get("bytes")
                .unwrap(),
            &DescriptionValueType::Bytes(bytes.clone())
        );

        assert_eq!(
            agent_description
                .non_identifying_attributes
                .get("another bool")
                .unwrap(),
            &DescriptionValueType::Bool(false)
        );

        assert_eq!(
            agent_description
                .identifying_attributes
                .get("float")
                .unwrap(),
            &DescriptionValueType::Float(5.6)
        );

        assert_eq!(
            agent_description
                .non_identifying_attributes
                .get("another float")
                .unwrap(),
            &DescriptionValueType::Float(15.6)
        );

        assert_eq!(
            agent_description
                .non_identifying_attributes
                .get("bytes")
                .unwrap(),
            &DescriptionValueType::Bytes(bytes)
        );
    }

    #[test]
    fn test_populate_agent_description() {
        let bytes: Vec<u8> = vec![
            1, 143, 243, 141, 1, 179, 119, 150, 178, 200, 28, 128, 105, 188, 106, 223,
        ];

        struct TestCase {
            name: String,
            expected: Vec<KeyValue>,
            agent_description_items: HashMap<String, DescriptionValueType>,
        }
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                name: "empty".to_string(),
                expected: Vec::new(),
                agent_description_items: HashMap::new(),
            },
            TestCase {
                name: "multiple values".to_string(),
                expected: vec![
                    KeyValue {
                        key: "string val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::StringValue("a string".to_string())),
                        }),
                    },
                    KeyValue {
                        key: "int val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::IntValue(5)),
                        }),
                    },
                    KeyValue {
                        key: "bool val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::BoolValue(false)),
                        }),
                    },
                    KeyValue {
                        key: "float val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::DoubleValue(6.7)),
                        }),
                    },
                    KeyValue {
                        key: "another bool val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::BoolValue(true)),
                        }),
                    },
                    KeyValue {
                        key: "bytes val".to_string(),
                        value: Some(AnyValue {
                            value: Some(Value::BytesValue(bytes.clone())),
                        }),
                    },
                ],
                agent_description_items: HashMap::from([
                    (
                        "string val".to_string(),
                        DescriptionValueType::String("a string".to_string()),
                    ),
                    ("int val".to_string(), DescriptionValueType::Int(5)),
                    ("bool val".to_string(), DescriptionValueType::Bool(false)),
                    ("float val".to_string(), DescriptionValueType::Float(6.7)),
                    (
                        "another bool val".to_string(),
                        DescriptionValueType::Bool(true),
                    ),
                ]),
            },
        ];

        let sort_by_key = |a: &KeyValue, b: &KeyValue| a.key.cmp(&b.key);

        for mut test_case in test_cases {
            let mut description = populate_agent_description(test_case.agent_description_items);
            assert_eq!(
                &description.sort_by(sort_by_key),
                &test_case.expected.sort_by(sort_by_key),
                "{} failed",
                test_case.name
            );
        }
    }

    #[test]
    fn test_agent_description_to_protobuf_conversion() {
        let bytes_identifying: Vec<u8> = vec![
            1, 143, 243, 141, 1, 179, 119, 150, 178, 200, 28, 128, 105, 188, 106, 223,
        ];
        let bytes_non_identifying: Vec<u8> = vec![1, 2, 3, 4];
        let agent_description = AgentDescription {
            identifying_attributes: HashMap::from([
                ("string".into(), "some string".to_string().into()),
                ("int".into(), 45.into()),
                ("bool".into(), true.into()),
                ("float".into(), 5.6.into()),
                ("bytes".into(), bytes_identifying.clone().into()),
            ]),
            non_identifying_attributes: HashMap::from([
                ("another string".into(), "another string value".into()),
                ("another int".into(), 145.into()),
                ("another bool".into(), false.into()),
                ("another float".into(), 15.6.into()),
                ("bytes".into(), bytes_non_identifying.clone().into()),
            ]),
        };

        let mut expected = ProtobufAgentDescription {
            identifying_attributes: vec![
                KeyValue {
                    key: "string".into(),
                    value: Some(AnyValue {
                        value: Some(StringValue("some string".into())),
                    }),
                },
                KeyValue {
                    key: "int".into(),
                    value: Some(AnyValue {
                        value: Some(IntValue(45)),
                    }),
                },
                KeyValue {
                    key: "bool".into(),
                    value: Some(AnyValue {
                        value: Some(BoolValue(true)),
                    }),
                },
                KeyValue {
                    key: "float".into(),
                    value: Some(AnyValue {
                        value: Some(DoubleValue(5.6)),
                    }),
                },
                KeyValue {
                    key: "bytes".into(),
                    value: Some(AnyValue {
                        value: Some(BytesValue(bytes_identifying.clone())),
                    }),
                },
            ],
            non_identifying_attributes: vec![
                KeyValue {
                    key: "another string".into(),
                    value: Some(AnyValue {
                        value: Some(StringValue("another string value".into())),
                    }),
                },
                KeyValue {
                    key: "another int".into(),
                    value: Some(AnyValue {
                        value: Some(IntValue(145)),
                    }),
                },
                KeyValue {
                    key: "another bool".into(),
                    value: Some(AnyValue {
                        value: Some(BoolValue(false)),
                    }),
                },
                KeyValue {
                    key: "another float".into(),
                    value: Some(AnyValue {
                        value: Some(DoubleValue(15.6)),
                    }),
                },
                KeyValue {
                    key: "bytes".into(),
                    value: Some(AnyValue {
                        value: Some(BytesValue(bytes_non_identifying.clone())),
                    }),
                },
            ],
        };

        let sort_by_key = |a: &KeyValue, b: &KeyValue| a.key.cmp(&b.key);

        let mut actual_proto_description: ProtobufAgentDescription = agent_description.into();

        expected.identifying_attributes.sort_by(sort_by_key);
        expected.non_identifying_attributes.sort_by(sort_by_key);

        actual_proto_description
            .identifying_attributes
            .sort_by(sort_by_key);
        actual_proto_description
            .non_identifying_attributes
            .sort_by(sort_by_key);

        assert_eq!(expected, actual_proto_description);
    }
}
