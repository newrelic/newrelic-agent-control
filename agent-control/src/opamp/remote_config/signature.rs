use opamp_client::opamp::proto::CustomMessage;
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::{collections::HashMap, fmt::Debug};
use thiserror::Error;

/// signature custom message capability
pub const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
/// signature custom message type
pub const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "newrelicRemoteConfigSignature";
// Supported signature algorithms
// RSA regex matching supported RSA signature algorithms, length between 2048 and 8192 bits
pub const RSA_REGEX: &str = "RSA_PKCS1_([0-9]+)_SHA(256|384|512)";
pub const RSA_PKCS1_2048_8192_SHA256: &str = "RSA_PKCS1_2048_8192_SHA256";
pub const RSA_PKCS1_2048_8192_SHA384: &str = "RSA_PKCS1_2048_8192_SHA384";
pub const RSA_PKCS1_2048_8192_SHA512: &str = "RSA_PKCS1_2048_8192_SHA512";
pub const ECDSA_P256_SHA256: &str = "ECDSA_P256_SHA256";
pub const ECDSA_P256_SHA384: &str = "ECDSA_P256_SHA384";
pub const ECDSA_P384_SHA256: &str = "ECDSA_P384_SHA256";
pub const ECDSA_P384_SHA384: &str = "ECDSA_P384_SHA384";
pub const ED25519: &str = "ED25519";

fn rsa_regex() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(RSA_REGEX).unwrap())
}

// TODO: simplify supported algorithms when removing support for certificate verification
/// Defines the supported algorithms for signing
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "&str")]
#[allow(non_camel_case_types)]
pub enum SigningAlgorithm {
    RSA_PKCS1_2048_8192_SHA256,
    RSA_PKCS1_2048_8192_SHA384,
    RSA_PKCS1_2048_8192_SHA512,
    ECDSA_P256_SHA256,
    ECDSA_P256_SHA384,
    ECDSA_P384_SHA256,
    ECDSA_P384_SHA384,
    ED25519,
}

impl TryFrom<&str> for SigningAlgorithm {
    type Error = SignatureError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if let Some(rsa_algorithm) = parse_rsa_algorithm(s) {
            return Ok(rsa_algorithm);
        }
        match s.to_uppercase().as_str() {
            ECDSA_P256_SHA256 => Ok(Self::ECDSA_P256_SHA256),
            ECDSA_P256_SHA384 => Ok(Self::ECDSA_P256_SHA384),
            ECDSA_P384_SHA256 => Ok(Self::ECDSA_P384_SHA256),
            ECDSA_P384_SHA384 => Ok(Self::ECDSA_P384_SHA384),
            ED25519 => Ok(Self::ED25519),
            _unsupported_algorithm => Err(SignatureError::UnsupportedAlgorithm(s.to_string())),
        }
    }
}

impl AsRef<str> for SigningAlgorithm {
    fn as_ref(&self) -> &str {
        match self {
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA256 => RSA_PKCS1_2048_8192_SHA256,
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA384 => RSA_PKCS1_2048_8192_SHA384,
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512 => RSA_PKCS1_2048_8192_SHA512,
            SigningAlgorithm::ECDSA_P256_SHA256 => ECDSA_P256_SHA256,
            SigningAlgorithm::ECDSA_P256_SHA384 => ECDSA_P256_SHA384,
            SigningAlgorithm::ECDSA_P384_SHA256 => ECDSA_P384_SHA256,
            SigningAlgorithm::ECDSA_P384_SHA384 => ECDSA_P384_SHA384,
            SigningAlgorithm::ED25519 => ED25519,
        }
    }
}

impl TryFrom<&SigningAlgorithm> for &webpki::SignatureAlgorithm {
    type Error = SignatureError;
    fn try_from(value: &SigningAlgorithm) -> Result<Self, Self::Error> {
        let algorithm = match value {
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA256 => &webpki::RSA_PKCS1_2048_8192_SHA256,
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA384 => &webpki::RSA_PKCS1_2048_8192_SHA384,
            SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512 => &webpki::RSA_PKCS1_2048_8192_SHA512,
            SigningAlgorithm::ECDSA_P256_SHA256 => &webpki::ECDSA_P256_SHA256,
            SigningAlgorithm::ECDSA_P256_SHA384 => &webpki::ECDSA_P256_SHA384,
            SigningAlgorithm::ECDSA_P384_SHA256 => &webpki::ECDSA_P384_SHA256,
            SigningAlgorithm::ECDSA_P384_SHA384 => &webpki::ECDSA_P384_SHA384,
            SigningAlgorithm::ED25519 => &webpki::ED25519,
        };
        Ok(algorithm)
    }
}

// parses the RSA algorithm string coming from the server into a supported signature algorithm
// example: "RSA_PKCS1_2048_SHA256" -> RSA_PKCS1_2048_8192_SHA256
// example: "RSA_PKCS1_4444_SHA256" -> RSA_PKCS1_2048_8192_SHA256
fn parse_rsa_algorithm(algo: &str) -> Option<SigningAlgorithm> {
    let m = rsa_regex().captures(algo.as_bytes())?;
    let (_, [lenght_bytes, hash_bytes]) = m.extract();

    // Validate the length is within the supported range
    let length = std::str::from_utf8(lenght_bytes)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())?;
    if !(2048..=8192).contains(&length) {
        return None;
    }

    match hash_bytes {
        b"256" => Some(SigningAlgorithm::RSA_PKCS1_2048_8192_SHA256),
        b"384" => Some(SigningAlgorithm::RSA_PKCS1_2048_8192_SHA384),
        b"512" => Some(SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512),
        _ => None,
    }
}

/// In order to mitigate MITM attacks, the OpAMP server signs the remote configuration and sends the
/// signature data as part of a CustomMessage in the same ServerToAgent message where the RemoteConfig is sent.
/// Agent control will verify that the signature and the configuration data match. `SignatureValidator` is
/// responsible for verifying the signature with the certificate fetched from the server.
///
/// The signature will consist in a encrypted hash of the configuration data, signed with a private key.
/// The public key is distributed to the agents in the form of a HTTPS server TLS certificate.
///
/// Example:
/// ```json
/// ServerToAgent: {
/// remote_config:{
///     config: {
///           config_map: {
///                 "agentConfig": {
///                       body: "chart_version: 1.10.12\nchart_values:\n  podLabels: \"192.168.5.0\""
///                       content_type: ""
///                 }
///           }
///     }
///     config_hash: "817982697f614312018935c351fdd71aca190f106fda2d7bc69da86e878ea5e4"
/// }
/// custom_message:{
///     capability: "com.newrelic.security.configSignature"
///     type: "newrelicRemoteConfigSignature"
///     data: {
///           "agentConfig": [{
///                 "checksum":  "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
///                 "checksumAlgorithm":  "SHA256",
///                 "signature":  "nppw2CuZg+YO5MsEoNOsHlgHxF7qAwWPli37NGXAr5isfP1jUTSJcLi0l7k9lNlpbq31GF9DZ0JQBZhoGS0j+sDjvirKSb7yXdqj6JcZ8sxax7KWAnk5QPiwLHFA1kGmszVJ/ccbwtVozG46FvKedcc3X5RME/HGdJupKBe3UzmJawL0xs9jNY+9519CL+CpbkBl/WgCvrIUhTNZv5TUHK23hMD+kz1Brf60pW7MQVtsyClOllsb6WhAsSXdhkpSCJ+96ZGyYywUlvx3/vkBM5a7q4IWqiPM4U0LPZDMQJQCCpxWV3T7cnIR1Ye2yYUqJHs9vfKmTWeBKH2Tb5FgpQ==",
///                 "signingAlgorithm": "RSA_PKCS1_2048_SHA256",
///                 "signatureSpecification": "PKCS #1 v2.2",
///                 "signingDomain": "iast-csec-se.test-poised-pear.cell.us.nr-data.net",
///                 "keyId":  "778b223984d389ad6555bdbbbf118420290c53296b6511e1964309965ec5f710"
///           }]
///     }
/// }
/// }
/// ```
/// `Signatures` holds the signature custom message data. It is coupled to a RemoteConfig message and
/// should be present in the same ServerToAgent message.
///
/// Even if each config identifier may contain many signature details (it holds an array) it is deserialized
/// as a single structure of [SignatureData] containing the first signature with a supported algorithm.
///
/// Example:
/// ```
/// use crate::newrelic_agent_control::opamp::remote_config::signature::Signatures;
///
/// let data= r#"{
///      "agentConfig": [
///         {
///            "signature":  "some signature",
///            "signingAlgorithm": "UNSUPPORTED",
///            "keyId":  "some key id"
///         },
///         {
///            "signature":  "some signature",
///            "signingAlgorithm": "ED25519",
///            "keyId":  "some key id"
///         },
///         {
///            "signature":  "some signature",
///            "signingAlgorithm": "RSA_PKCS1_2048_SHA256",
///            "keyId":  "some key id"
///         }
///     ]
/// }"#.as_bytes().to_vec();
///
/// let signatures: Signatures = serde_json::from_slice(&data).unwrap();
/// let (_, signature) = signatures.signatures.iter().next().unwrap();
/// assert_eq!(signature.signing_algorithm.as_ref(), "ED25519");
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
    /// Signing algorithm used the config:
    /// [ECDSA_P256_SHA256,ECDSA_P256_SHA384,ECDSA_P384_SHA256,ECDSA_P384_SHA384,RSA_PKCS1_[2048-8192]_SHA256,
    /// RSA_PKCS1_2048_8192_SHA384,RSA_PKCS1_2048_8192_SHA512,RSA_PKCS1_3072_8192_SHA384,ED25519]
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
    use crate::opamp::remote_config::signature::ECDSA_P256_SHA256;
    use crate::opamp::remote_config::signature::ECDSA_P256_SHA384;
    use crate::opamp::remote_config::signature::ED25519;
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
                          "3936250589": [{
                                "checksum":  "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                                "checksumAlgorithm":  "SHA256",
                                "signature":  "nppw2CuZg+YO5MsEoNOsHlgHxF7qAwWPli37NGXAr5isfP1jUTSJcLi0l7k9lNlpbq31GF9DZ0JQBZhoGS0j+sDjvirKSb7yXdqj6JcZ8sxax7KWAnk5QPiwLHFA1kGmszVJ/ccbwtVozG46FvKedcc3X5RME/HGdJupKBe3UzmJawL0xs9jNY+9519CL+CpbkBl/WgCvrIUhTNZv5TUHK23hMD+kz1Brf60pW7MQVtsyClOllsb6WhAsSXdhkpSCJ+96ZGyYywUlvx3/vkBM5a7q4IWqiPM4U0LPZDMQJQCCpxWV3T7cnIR1Ye2yYUqJHs9vfKmTWeBKH2Tb5FgpQ==",
                                "signingAlgorithm": "RSA_PKCS1_2048_SHA256",
                                "signatureSpecification": "PKCS #1 v2.2",
                                "signingDomain": "iast-csec-se.test-poised-pear.cell.us.nr-data.net",
                                "keyId":  "778b223984d389ad6555bdbbbf118420290c53296b6511e1964309965ec5f710"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::RSA_PKCS1_2048_8192_SHA256,
            },
            TestCase {
                name: "required fields only, RSA_PKCS1_2048_SHA256",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "RSA_PKCS1_2048_SHA256",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::RSA_PKCS1_2048_8192_SHA256,
            },
            TestCase {
                name: "RSA_PKCS1_2048_SHA512",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "RSA_PKCS1_2048_SHA512",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512,
            },
            TestCase {
                name: "RSA_PKCS1_2049_SHA512",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "RSA_PKCS1_2049_SHA512",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512,
            },
            TestCase {
                name: "RSA_PKCS1_3072_SHA384",
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "RSA_PKCS1_3072_SHA384",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::RSA_PKCS1_2048_8192_SHA384,
            },
            TestCase {
                name: ECDSA_P256_SHA256,
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "ECDSA_P256_SHA256",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::ECDSA_P256_SHA256,
            },
            TestCase {
                name: ECDSA_P256_SHA384,
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "ECDSA_P256_SHA384",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
                },
                algorithm: SigningAlgorithm::ECDSA_P256_SHA384,
            },
            TestCase {
                name: ED25519,
                custom_message: CustomMessage {
                    capability: super::SIGNATURE_CUSTOM_CAPABILITY.to_string(),
                    r#type: super::SIGNATURE_CUSTOM_MESSAGE_TYPE.to_string(),
                    data: r#"{
                          "3936250589": [{
                                "signature":  "fake",
                                "signingAlgorithm": "ED25519",
                                "keyId":  "fake"
                          }]
                    }"#.as_bytes().to_vec(),
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
                    }"#.as_bytes().to_vec(),
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
            SigningAlgorithm::ECDSA_P256_SHA256
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
