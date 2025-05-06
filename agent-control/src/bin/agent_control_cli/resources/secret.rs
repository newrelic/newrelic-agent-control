use std::{collections::BTreeMap, fmt::Display};

use k8s_openapi::ByteString;
use kube::api::{DynamicObject, ObjectMeta, TypeMeta};
use tracing::{debug, info};

use crate::{errors::ParseError, utils::parse_key_value_pairs};

pub struct SecretData {
    /// Object name
    pub name: String,

    /// The type of the secret
    pub secret_type: SecretType,

    /// Enable/disable modification of the secret
    pub immutable: Option<bool>,

    /// Data contains the secret data. Each key must consist of alphanumeric characters, '-', '_' or '.'. The serialized form of the secret data is a base64 encoded string, representing the arbitrary (possibly non-string) data value here. Described in https://tools.ietf.org/html/rfc4648#section-4
    pub data: Option<BTreeMap<String, ByteString>>,

    /// stringData allows specifying non-binary secret data in string form. It is provided as a write-only input field for convenience. All keys and values are merged into the data field on write, overwriting any existing values. The stringData field is never output when reading from the API.
    pub string_data: Option<BTreeMap<String, String>>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    pub labels: Option<String>,

    /// Non-identifying metadata
    pub annotations: Option<String>,
}

/// Different types of secrets that can be created in kubernetes.
///
/// Check the [Kubernetes documentation](https://kubernetes.io/docs/concepts/configuration/secret/#secret-types)
pub enum SecretType {
    Opaque,
    ServiceAccountToken,
    DockerCfg,
    DockerConfigJson,
    BasicAuth,
    SshAuth,
    Tls,
    Token,
}

impl Display for SecretType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let type_string = match self {
            SecretType::Opaque => "Opaque",
            SecretType::ServiceAccountToken => "kubernetes.io/service-account-token	",
            SecretType::DockerCfg => "kubernetes.io/dockercfg",
            SecretType::DockerConfigJson => "kubernetes.io/dockerconfigjson	",
            SecretType::BasicAuth => "kubernetes.io/basic-auth",
            SecretType::SshAuth => "kubernetes.io/ssh-auth",
            SecretType::Tls => "kubernetes.io/tls",
            SecretType::Token => "bootstrap.kubernetes.io/token",
        };

        write!(f, "{}", type_string)
    }
}

impl TryFrom<SecretData> for DynamicObject {
    type Error = ParseError;

    fn try_from(value: SecretData) -> Result<Self, Self::Error> {
        info!("Creating Secret object representation");

        let labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(value.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let mut data = serde_json::json!({
            "type": value.secret_type.to_string(),
        });
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
            types: Some(TypeMeta {
                api_version: "v1".to_string(),
                kind: "Secret".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(value.name.clone()),
                labels,
                annotations,
                ..Default::default()
            },
            data,
        };
        info!("Secret object representation created");

        Ok(dynamic_object)
    }
}

#[cfg(test)]
mod tests {
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
                "immutable": true,
            }),
        };

        let secret_data = SecretData {
            name: "test-secret".to_string(),
            secret_type: SecretType::Opaque,
            immutable: Some(true),
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
            labels: Some("label1=value1,label2=value2".to_string()),
            annotations: Some("annotation1=value1,annotation2=value2".to_string()),
        };
        let actual_dynamic_object = DynamicObject::try_from(secret_data).unwrap();

        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
