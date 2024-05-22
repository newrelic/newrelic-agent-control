pub mod authenticator;
pub mod token;
pub mod token_retriever;

use crate::token::Token;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TokenRetrieverError {
    #[error("not defined yet")]
    NotDefinedYetError,
}

/// The TokenRetriever will be the responsible to retrieve an authorization token
pub trait TokenRetriever {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
}
