use crate::opamp::instance_id::getter::InstanceIDError;
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};
use std::fmt::{Display, Formatter};
use uuid::Uuid;

// InstanceID holds the to_string of Uuid assigned to a Agent
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Eq, Hash)]
pub struct InstanceID(Uuid);

impl InstanceID {
    // Creates a new instanceID with a random valid value. Use try_from methods
    // to build this struct with a static value.
    pub fn create() -> Self {
        Self(Uuid::now_v7())
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
        Ok(Self(Uuid::parse_str(value).map_err(|e| {
            InstanceIDError::InvalidFormat(e.to_string())
        })?))
    }
}

impl From<InstanceID> for Vec<u8> {
    fn from(val: InstanceID) -> Self {
        val.0.into()
    }
}

impl Display for InstanceID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
