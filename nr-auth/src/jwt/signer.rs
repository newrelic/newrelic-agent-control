use super::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};

mod local;

pub trait JwtSigner {
    // should Claims be single-use? Local implementation only needs references but
    // perhaps consuming it is useful from the security/safety perspective?
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError>;
}
