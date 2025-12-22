use std::fmt::{Debug, Display, Formatter};
pub mod k8s;

#[derive(Debug, Clone, PartialEq)]
pub struct EntityGuid {
    pub guid: String,
    pub opamp_field: String,
}

#[derive(thiserror::Error, Debug)]
pub struct GuidCheckError(pub String);

impl Display for GuidCheckError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait GuidChecker {
    fn check_guid(&self) -> Result<EntityGuid, GuidCheckError>;
}
