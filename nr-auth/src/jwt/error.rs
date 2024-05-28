use jsonwebtoken::errors;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JwtEncoderError {
    #[error("unable to encode token: `{0}`")]
    TokenEncoding(#[from] errors::Error),
    #[error("invalid timestamp: `{0}`")]
    InvalidTimestamp(String),
}
