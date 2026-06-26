//! Error type for the Kubernetes CLI commands.
use thiserror::Error;

use crate::cli::common::error::CliError;

/// Errors that can occur while running a Kubernetes CLI command.
#[derive(Debug, Error)]
pub enum K8sCliError {
    /// The Kubernetes client could not be created.
    #[error("failed to create k8s client: {0}")]
    K8sClient(String),

    /// Applying a resource to the cluster failed.
    #[error("failed to apply resource: {0}")]
    ApplyResource(String),

    /// Reading a resource from the cluster failed.
    #[error("failed to get resource: {0}")]
    GetResource(String),

    /// The post-install health check failed.
    #[error("installation check failure: {0}")]
    InstallationCheck(String),

    /// Deleting a resource from the cluster failed.
    #[error("failed to delete resource: {0}")]
    DeleteResource(String),

    /// Any other error not covered by the variants above.
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
