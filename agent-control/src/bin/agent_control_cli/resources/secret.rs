use k8s_openapi::api::core::v1::Secret;
use kube::api::{DynamicObject, TypeMeta};
use tracing::info;

use crate::errors::ParseError;

pub struct SecretData(pub Secret);

impl TryFrom<SecretData> for DynamicObject {
    type Error = ParseError;

    fn try_from(value: SecretData) -> Result<Self, Self::Error> {
        let value = value.0;
        let name = value.metadata.clone().name.unwrap_or("unknown".to_string());

        info!("Creating Secret representation with name \"{}\"", name);

        let mut data = serde_json::json!({});
        if let Some(secret_type) = value.type_ {
            data["type"] = serde_json::json!(secret_type.to_string());
        }
        if let Some(data_map) = value.data {
            data["data"] = serde_json::json!(data_map);
        }
        if let Some(string_data_map) = value.string_data {
            data["stringData"] = serde_json::json!(string_data_map);
        }
        if let Some(immutable) = value.immutable {
            data["immutable"] = serde_json::json!(immutable);
        }

        let dynamic_object = DynamicObject {
            types: Some(secret_type_meta()),
            metadata: value.metadata,
            data,
        };

        info!(
            "Helm repository representation with name \"{}\" created",
            name
        );

        Ok(dynamic_object)
    }
}

pub fn secret_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "v1".to_string(),
        kind: "Secret".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::ByteString;
    use kube::api::ObjectMeta;

    use super::*;

    #[test]
    fn test_to_dynamic_object() {
        let expected_dynamic_object = DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".to_string(),
                kind: "Secret".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                labels: Some(
                    vec![
                        ("label1".to_string(), "value1".to_string()),
                        ("label2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                annotations: Some(
                    vec![
                        ("annotation1".to_string(), "value1".to_string()),
                        ("annotation2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            data: serde_json::json!({
                "type": "Opaque",
                "data": {
                    "key1": ByteString(vec![1, 2, 3]),
                    "key2": ByteString(vec![4, 5, 6]),
                },
                "stringData": {
                    "key3": "value3".to_string(),
                    "key4": "value4".to_string(),
                },
            }),
        };

        let secret_data = SecretData(Secret {
            type_: Some("Opaque".to_string()),
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                labels: Some(
                    vec![
                        ("label1".to_string(), "value1".to_string()),
                        ("label2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                annotations: Some(
                    vec![
                        ("annotation1".to_string(), "value1".to_string()),
                        ("annotation2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            data: Some(
                vec![
                    ("key1".to_string(), ByteString(vec![1, 2, 3])),
                    ("key2".to_string(), ByteString(vec![4, 5, 6])),
                ]
                .into_iter()
                .collect(),
            ),
            string_data: Some(
                vec![
                    ("key3".to_string(), "value3".to_string()),
                    ("key4".to_string(), "value4".to_string()),
                ]
                .into_iter()
                .collect(),
            ),
            immutable: None,
        });
        let actual_dynamic_object = DynamicObject::try_from(secret_data).unwrap();

        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
