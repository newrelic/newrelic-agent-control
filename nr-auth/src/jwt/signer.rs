use super::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};
use local::{LocalPrivateKeySigner, LocalPrivateKeySignerError};
use thiserror::Error;

pub mod local;

/// A JWT signer.
pub trait JwtSigner {
    // should Claims be single-use? Local implementation only needs references but
    // perhaps consuming it is useful from the security/safety perspective?
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError>;
}

/// Enumerates all implementations for `JwtSigner` for static dispatching reasons.
pub enum JwtSignerImpl {
    Local(LocalPrivateKeySigner),
}

#[cfg_attr(test, mockall::automock)]
impl JwtSigner for JwtSignerImpl {
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError> {
        match self {
            Self::Local(local_signer) => local_signer.sign(claims),
        }
    }
}

#[derive(Error, Debug)]
pub enum JwtSignerImplError {
    #[error("building local private key JWT signer: `{0}`")]
    LocalPrivateKeySignerError(#[from] LocalPrivateKeySignerError),
}
