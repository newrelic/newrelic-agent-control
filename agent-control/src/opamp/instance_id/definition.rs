use opamp_client::operation::instance_uid::InstanceUid;
use serde::{Deserialize, Serialize, Serializer};
use std::{convert::TryInto, fmt::Display};

/// Holds an OpAMP's instance uid and easy its serialization/deserialization.
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct InstanceID(InstanceUid);

impl InstanceID {
    /// Creates a new instance id.
    pub fn create() -> Self {
        Self(InstanceUid::create())
    }
}

// Use the underlying instance uid string representation when serializing
impl Serialize for InstanceID {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

// Deserialize from string
impl<'de> Deserialize<'de> for InstanceID {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string_value = String::deserialize(deserializer)?;
        Ok(Self(
            string_value.try_into().map_err(serde::de::Error::custom)?,
        ))
    }
}

impl From<InstanceID> for InstanceUid {
    fn from(val: InstanceID) -> Self {
        val.0
    }
}

impl From<InstanceUid> for InstanceID {
    fn from(val: InstanceUid) -> Self {
        Self(val)
    }
}

impl From<InstanceID> for Vec<u8> {
    fn from(value: InstanceID) -> Self {
        value.0.into()
    }
}

impl Display for InstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::InstanceID;
    use opamp_client::operation::instance_uid::InstanceUid;

    #[test]
    fn test_instance_id_serialize_deserialize() {
        let id_as_str = "0190592A82877FB1A6D91ECAA57032BD";
        let unserlialized: InstanceID = serde_yaml::from_str(id_as_str).unwrap();
        assert_eq!(
            InstanceUid::from(unserlialized.clone()).to_string(),
            String::from(id_as_str)
        );
        let serialized = serde_yaml::to_string(&unserlialized).unwrap();
        assert_eq!(serialized, format!("{}\n", String::from(id_as_str))) // string yaml serialization ends with \n
    }
}
