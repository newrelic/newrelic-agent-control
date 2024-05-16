use crate::k8s::Error as K8sError;

#[derive(thiserror::Error, Debug)]
pub enum HealthCheckerError {
    #[error("{0}")]
    Generic(String),
    // TODO: actually use the error variants below
    #[error("The invalid or missing field `{0}` in `{1}`")]
    InvalidField(String, String),
    #[error("Error fetching k8s object {0}")]
    K8sError(#[from] K8sError),
}
