use super::{claims::Claims, error::JwtEncoderError, signed::SignedJwt};

mod local;

pub trait JwtSigner {
    fn sign(&self, claims: Claims) -> Result<SignedJwt, JwtEncoderError>;
}
