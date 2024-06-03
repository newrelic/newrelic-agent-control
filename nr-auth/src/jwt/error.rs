use thiserror::Error;

#[derive(Error, Debug)]
pub enum JwtEncoderError {
    // Generic error for each implementation to use
    #[error("unable to encode token: `{0}`")]
    TokenEncoding(String),
}
