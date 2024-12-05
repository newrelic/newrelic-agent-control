use serde::{Deserialize, Serialize, Serializer};
use std::convert::{TryFrom, TryInto};
use std::fmt::{Display, Formatter};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum InstanceIDError {
    #[error("invalid InstanceID format: `{0}`")]
    InvalidFormat(String),
}

// InstanceID holds the to_string of Uuid assigned to a Agent
#[derive(Debug, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct InstanceID(Uuid);

impl InstanceID {
    // Creates a new instanceID with a random valid value. Use try_from methods
    // to build this struct with a static value.
    pub fn create() -> Self {
        Self(Uuid::now_v7())
    }
}

// use Display when serializing so InstanceID is always serialized in the same format
impl Serialize for InstanceID {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl TryFrom<String> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TryFrom<Vec<u8>> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let uuid: Uuid = value
            .try_into()
            .map_err(|e: uuid::Error| InstanceIDError::InvalidFormat(e.to_string()))?;

        Ok(Self(uuid))
    }
}

impl TryFrom<&str> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(value).map_err(|e| {
            InstanceIDError::InvalidFormat(e.to_string())
        })?))
    }
}

impl From<InstanceID> for Vec<u8> {
    fn from(val: InstanceID) -> Self {
        val.0.into()
    }
}

/// InstanceID represents
/// an [Agent Instance ID](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#agenttoserverinstance_uid)
///
/// Currently, it's represented by
/// a [UUID v7](https://www.ietf.org/archive/id/draft-ietf-uuidrev-rfc4122bis-14.html#name-uuid-version-7)
///
/// For a matter of simplicity in the API it has been agreed with the Fleet Management Team
/// that the String representation of an InstanceID will be uppercase and without hyphens
///
/// 0190592a-8287-7fb1-a6d9-1ecaa57032bd
///
/// Will be represented as:
///
/// 0190592A82877FB1A6D91ECAA57032BD
///
/// For the communication we will use the Bytes format.
///
/// The used crate ([uuid](https://github.com/uuid-rs/uuid/blob/1.10.0/src/fmt.rs#L72)) already supports this format:
///
/// ```rust
/// use uuid::fmt::Hyphenated;
/// use uuid::Uuid;
///
/// let uuid = Uuid::now_v7();
/// //Format a [`Uuid`] as a hyphenated string, like
/// // `67e55044-10b1-426f-9247-bb680e5fe0c8`
/// let hyphenated = uuid.as_hyphenated();
/// // Format a [`Uuid`] as a simple string, like
///  // `67e5504410b1426f9247bb680e5fe0c8`.
/// let simple = uuid.as_simple();
/// ```
impl Display for InstanceID {
    // Format agreed with Fleet Management: Uppercase and no hyphens
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_simple().to_string().to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::InstanceID;
    use super::InstanceIDError;

    use assert_matches::assert_matches;

    #[test]
    fn test_instance_id_display_uppercase_no_hyphen() {
        let instance_id = InstanceID::try_from("0190592a-8287-7fb1-a6d9-1ecaa57032bd").unwrap();
        assert_eq!(
            instance_id.to_string(),
            String::from("0190592A82877FB1A6D91ECAA57032BD")
        );
    }

    #[test]
    fn test_instance_id_accepts_no_hyphen_on_build() {
        let instance_id = InstanceID::try_from("0190592A82877FB1A6D91ECAA57032BD").unwrap();
        assert_eq!(
            instance_id.to_string(),
            String::from("0190592A82877FB1A6D91ECAA57032BD")
        );
    }

    #[test]
    fn test_instance_id_invalid_uuid() {
        struct TestCase {
            _name: &'static str,
            uuid: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let err = InstanceID::try_from(self.uuid).unwrap_err();
                assert_matches!(err, InstanceIDError::InvalidFormat(_))
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "Contains a non-hexadecimal character 'g'",
                uuid: "g2345678-1234-7234-1234-123456789012",
            },
            TestCase {
                _name: "Hyphens in the wrong position",
                uuid: "123456781234-7234-1234-123456789012",
            },
            TestCase {
                _name: "Incorrect length, too short",
                uuid: "12345678-1234-7234-1234-12345678901",
            },
            TestCase {
                _name: "Incorrect length, too long",
                uuid: "12345678-1234-7234-1234-1234567890123",
            },
            TestCase {
                _name: "Ends with a hyphen",
                uuid: "12345678-1234-7234-1234-12345678901-",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
