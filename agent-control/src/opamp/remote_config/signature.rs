use opamp_client::opamp::proto::CustomMessage;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};
use thiserror::Error;

/// signature custom message capability
pub const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
/// signature custom message type
pub const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "newrelicRemoteConfigSignature";
// Supported signature algorithms
pub const ED25519: &str = "ED25519";

/// Defines the supported algorithms for signing
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "&str")]
pub enum SigningAlgorithm {
    ED25519,
}

impl TryFrom<&str> for SigningAlgorithm {
    type Error = SignatureError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_uppercase().as_str() {
            ED25519 => Ok(Self::ED25519),
            _unsupported_algorithm => Err(SignatureError::UnsupportedAlgorithm(s.to_string())),
        }
    }
}

impl AsRef<str> for SigningAlgorithm {
    fn as_ref(&self) -> &str {
        match self {
            SigningAlgorithm::ED25519 => ED25519,
        }
    }
}

/// Holds the signature custom message data. It is coupled to a RemoteConfig message and
/// should be present in the same ServerToAgent message.
///
/// Even if each config identifier may contain many signature details (it holds an array) it is deserialized
/// as a single structure of [SignatureData] containing the first signature with a supported algorithm.
///
/// In order to mitigate MITM attacks, the OpAMP server signs the remote configuration and sends the
/// signature data as part of a CustomMessage in the same ServerToAgent message where the RemoteConfig is sent.
/// Agent control will verify that the signature and the configuration data match. `SignatureValidator` is
/// responsible for verifying the signature with the certificate fetched from the server.
///
/// The signed message is consist in the remote config standard encoded base64 sha256 of the config body, which is signed
/// with the private key and algorithm specified in the custom_message.
/// Public key is distributed in JWKS format in the following endpoints:
/// https://staging-publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
/// https://publickeys.eu.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
/// https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
///
/// Example:
/// ```json
/// ServerToAgent: {
/// remote_config:{
///     config: {
///           config_map: {
///                 "agentConfig": {
///                       body: "chart_version: \"6.0.1\""
///                       content_type: ""
///                 }
///           }
///     }
///     config_hash: "b5c6779371b3b1e608b55d0b0b4d970afa1d97f176d60cc8a7034b2b2d12da66"
/// }
/// custom_message:{
///     capability: "com.newrelic.security.configSignature"
///     type: "newrelicRemoteConfigSignature"
///     data: {
///           "agentConfig": [{
///                 "signature":  "YHUmpyXFyCw9LP4NbpGtBpY7u9iu0zWpkGv0ePw4WA2sCSJqdYK3G2RRVgIjHcWlFNwX8p4Yc+CQdGBDvr5RCw==",
///                 "signingAlgorithm": "ED25519",
///                 "keyId":  "AgentConfiguration/0"
///           }]
///     }
/// }
/// }
/// ```
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct Signatures {
    #[serde(flatten)]
    pub signatures: HashMap<ConfigID, SignatureData>,
}

/// Traverse the list of signature fields and return the first valid signature data.
/// If no valid signature data is found, return an error with the accumulated errors.
fn parse_first_valid(
    signature_list: Vec<SignatureFields<String>>,
) -> Result<SignatureFields<SigningAlgorithm>, SignatureError> {
    let mut errors_accumulator = String::new();
    for (i, raw_signature) in signature_list.into_iter().enumerate() {
        match SignatureData::try_from(raw_signature) {
            Ok(valid_signature) => return Ok(valid_signature),
            Err(err) => {
                errors_accumulator.push_str(&format!(
                    "Cannot process the signature data in position {i}: {err}\n"
                ));
            }
        }
    }

    Err(SignatureError::InvalidData(errors_accumulator))
}

impl<'de> Deserialize<'de> for Signatures {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Externally, Signatures include an array of signature fields for algorithm backwards compatibility purposes
        type RawSignatures = HashMap<ConfigID, Vec<RawSignatureData>>;
        let raw_signatures = RawSignatures::deserialize(deserializer)?;

        // Get the first supported signature-data (SignatureData) for each config-map if any, return an error if there is
        // no valid signature data for any config_id.
        let mut signatures = HashMap::new();
        for (id, signature_list) in raw_signatures {
            let valid = parse_first_valid(signature_list).map_err(|e| {
                Error::custom(format!("there is no valid signature data for {id}: {e}"))
            })?;

            signatures.insert(id, valid);
        }

        Ok(Signatures { signatures })
    }
}

/// SignatureFields holds all the fields that make up the signature data. It allows us to represent the signature
/// data before validation ([RawSignatureData], where the signing algorithm is a string) and after validation
/// [SignatureData] (where the signing algorithm is represented by the [SigningAlgorithm] type).
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignatureFields<A> {
    /// RemoteConfiguration signature on TLS's `DigitallySigned.signature` format encoded in base64.
    pub signature: String,
    /// Public key identifier.
    pub key_id: String,
    /// Signing algorithm.
    pub signing_algorithm: A,
}

/// Represents the signature data before checking if the algorithm is supported.
type RawSignatureData = SignatureFields<String>;

/// Represent signature data ready to be used for config validation.
pub type SignatureData = SignatureFields<SigningAlgorithm>;

impl TryFrom<RawSignatureData> for SignatureData {
    type Error = SignatureError;

    fn try_from(s: RawSignatureData) -> Result<Self, Self::Error> {
        Ok(Self {
            signature: s.signature,
            key_id: s.key_id,
            signing_algorithm: s.signing_algorithm.as_str().try_into()?,
        })
    }
}

impl SignatureData {
    pub fn signature(&self) -> &[u8] {
        self.signature.as_bytes()
    }

    pub fn signature_algorithm(&self) -> &SigningAlgorithm {
        &self.signing_algorithm
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// Configuration identifier that corresponds to a specific remote configuration.
/// This key links signature data to its associated configuration in the remote config map.
pub type ConfigID = String;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum SignatureError {
    #[error("invalid config signature capability")]
    InvalidCapability,
    #[error("invalid config signature type")]
    InvalidType,
    #[error("invalid config signature data: {0}")]
    InvalidData(String),
    #[error("unsupported signature algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

impl TryFrom<&CustomMessage> for Signatures {
    type Error = SignatureError;

    fn try_from(custom_message: &CustomMessage) -> Result<Self, Self::Error> {
        if custom_message.capability != SIGNATURE_CUSTOM_CAPABILITY {
            return Err(SignatureError::InvalidCapability);
        }
        if custom_message.r#type != SIGNATURE_CUSTOM_MESSAGE_TYPE {
            return Err(SignatureError::InvalidType);
        }

        let signatures: Signatures = serde_json::from_slice(&custom_message.data)
            .map_err(|err| SignatureError::InvalidData(err.to_string()))?;

        Ok(signatures)
    }
}

#[cfg(test)]
mod tests {
    use super::SignatureData;
    use super::Signatures;
    use crate::opamp::remote_config::DEFAULT_AGENT_CONFIG_IDENTIFIER;
    use crate::opamp::remote_config::signature::SigningAlgorithm;
    use opamp_client::opamp::proto::CustomMessage;
    use std::collections::HashMap;

    impl Signatures {
        pub fn new_default(signature: &str, signing_algorithm: &str, key_id: &str) -> Self {
            Self {
                signatures: HashMap::from([(
                    DEFAULT_AGENT_CONFIG_IDENTIFIER.to_string(),
                    SignatureData::new(signature, signing_algorithm, key_id),
                )]),
            }
        }

        pub fn new_multiple(signatures: impl IntoIterator<Item = SignatureData>) -> Self {
            let signatures: HashMap<String, SignatureData> = signatures
                .into_iter()
                .enumerate()
                .map(|(k, signature)| (format!("{k}"), signature))
                .collect();
            Self { signatures }
        }
    }

    impl SignatureData {
        pub fn new(signature: &str, signing_algorithm: &str, key_id: &str) -> Self {
            Self {
                signing_algorithm: signing_algorithm.try_into().unwrap(),
                signature: signature.to_string(),
                key_id: key_id.to_string(),
            }
        }
    }

    #[test]
    fn test_deserialize_custom_message() {
        struct TestCase {
            name: &'static str,
            custom_message: CustomMessage,
            algorithm: SigningAlgorithm,
        }
        impl TestCase {
            fn run(self) {
                let signatures = Signatures::try_from(&self.custom_message)
                    .unwrap_or_else(|err| panic!("case: {} - {}", self.name, err));
                let (_, signature) = signatures.signatures.iter().next().unwrap();
                assert_eq!(signature.signing_algorithm, self.algorithm);
            }
        }
        let test_cases = vec![
            TestCase {
                name: "complete valid message",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "someConfigKey": [{
                                "signature":  "fake",
                                "signingAlgorithm": "ED25519",
                                "keyId":  "ac/0"
                          }]
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
                algorithm: SigningAlgorithm::ED25519,
            },
            TestCase {
                name: "Unsupported + ED25519",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "unsupported",
                                    "keyId":  "fake"
                                },
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "ED25519",
                                    "keyId":  "fake"
                                }
                          ]
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
                algorithm: SigningAlgorithm::ED25519,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_print_acc_err() {
        let custom_message = CustomMessage {
            capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
            r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
            data: r#"{
                          "1": [
                                {
                                    "signature":  "fake1",
                                    "signingAlgorithm": "unsupported1",
                                    "keyId":  "fake2"
                                },
                                {
                                    "signature":  "fake2",
                                    "signingAlgorithm": "unsupported2",
                                    "keyId":  "fake2"
                                }
                          ]
                    }"#
            .as_bytes()
            .to_vec(),
        };
        let error = Signatures::try_from(&custom_message).unwrap_err();
        assert!(error.to_string().contains("unsupported1"));
        assert!(error.to_string().contains("unsupported2"));
    }

    #[test]
    fn test_deserialize_signature_data_items_precedence() {
        let custom_message = CustomMessage {
            capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
            r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
            data: r#"{
                          "1": [
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "unsupported",
                                    "keyId":  "fake"
                                },
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "Ed25519",
                                    "keyId":  "fake"
                                },
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "ECDSA_P256_SHA256",
                                    "keyId":  "fake"
                                }
                          ],
                          "2": [
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "unsupported",
                                    "keyId":  "fake"
                                },
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "ECDSA_P256_SHA256",
                                    "keyId":  "fake"
                                },
                                {
                                    "signature":  "fake",
                                    "signingAlgorithm": "ED25519",
                                    "keyId":  "fake"
                                }
                          ]
                    }"#
            .as_bytes()
            .to_vec(),
        };
        let signatures = Signatures::try_from(&custom_message).unwrap();
        assert_eq!(
            signatures.signatures.get("1").unwrap().signing_algorithm,
            SigningAlgorithm::ED25519
        );
        assert_eq!(
            signatures.signatures.get("2").unwrap().signing_algorithm,
            SigningAlgorithm::ED25519
        );
    }

    #[test]
    fn test_deserialize_invalid_signature_data() {
        struct TestCase {
            name: &'static str,
            custom_message: CustomMessage,
        }
        impl TestCase {
            fn run(self) {
                let err = Signatures::try_from(&self.custom_message)
                    .expect_err(format!("case: {}", self.name).as_str());
                assert!(format!("{err:?}").contains("no valid signature data"));
            }
        }
        let test_cases = vec![
            TestCase {
                name: "unknown",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "unknown",
                                "keyId":  "fake"
                          }]
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
            },
            TestCase {
                name: "rsa invalid length",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "RSA_PKCS1_8193_SHA512",
                                "keyId":  "fake"
                          }]
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
            },
            TestCase {
                name: "No data",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": []
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
            },
            TestCase {
                name: "One config_id with no data",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "config_id1": [],
                          "config_id2": [{
                                "signature":  "fake",
                                "signingAlgorithm": "ED25519",
                                "keyId":  "fake"
                          }]
                    }"#
                    .as_bytes()
                    .to_vec(),
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
