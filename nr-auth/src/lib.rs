pub mod authenticator;
pub mod http_client;
pub mod jwt;
pub mod token;
pub mod token_retriever;

use crate::token::Token;
use thiserror::Error;

pub type ClientID = String;

#[derive(Error, Debug)]
pub enum TokenRetrieverError {
    #[error("retrieving token: `{0}`")]
    TokenRetrieverError(String),
    #[error("signing JWT: `{0}`")]
    JwtSignerError(#[from] jwt::error::JwtEncoderError),
    #[error("fetching access token: `{0}`")]
    AuthenticatorError(#[from] authenticator::AuthenticateError),
    #[error("acquiring cache mutex lock")]
    PoisonError,
}

/// The TokenRetriever will be the responsible to retrieve an authorization token
pub trait TokenRetriever {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
}
