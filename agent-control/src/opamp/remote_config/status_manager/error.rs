use thiserror::Error;

#[derive(Debug, Error, PartialEq, Clone)]
pub enum ConfigStatusManagerError {
    #[error("while retrieving the remote config status of the agent: `{0}`")]
    Retrieval(String),
    #[error("while storing the remote config status of the agent: `{0}`")]
    Store(String),
    #[error("while deleting the remote config status of the agent: `{0}`")]
    Deletion(String),
}
