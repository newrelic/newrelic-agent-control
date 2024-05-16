#[derive(Debug, thiserror::Error)]
pub enum HealthCheckerError {
    #[error("{0}")]
    Generic(String),
}
