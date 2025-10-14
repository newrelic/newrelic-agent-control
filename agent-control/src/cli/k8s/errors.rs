use thiserror::Error;

use crate::cli::error::CliError;

#[derive(Debug, Error)]
pub enum K8sCliError {
    #[error("failed to create k8s client: {0}")]
    K8sClient(String),

    #[error("failed to apply resource: {0}")]
    ApplyResource(String),

    #[error("failed to get resource: {0}")]
    GetResource(String),

    #[error("installation check failure: {0}")]
    InstallationCheck(String),

    #[error("failed to delete resource: {0}")]
    DeleteResource(String),

    #[error("{0}")]
    Generic(String),
}

impl From<K8sCliError> for CliError {
    fn from(value: K8sCliError) -> Self {
        match value {
            K8sCliError::K8sClient(_) => CliError::Precondition(value.to_string()),
            _ => CliError::Command(value.to_string()),
        }
    }
}
