use std::fmt::{Debug, Display, Formatter};

pub mod handler;
pub mod k8s;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentStatus {
    pub status: String,
    pub opamp_field: String,
}

#[derive(Debug)]
pub struct StatusCheckError(pub String);

impl Display for StatusCheckError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait StatusChecker {
    fn check_status(&self) -> Result<AgentStatus, StatusCheckError>;
}
