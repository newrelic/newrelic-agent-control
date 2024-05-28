use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// JWT ID. Required.
    pub(crate) jti: Ulid,
    /// Audience. Optional.
    pub(crate) aud: String,
    /// Expiration time (as UTC timestamp). Required.
    pub(crate) exp: u64,
    /// Issued at (as UTC timestamp). Optional.
    pub(crate) iat: u64,
    /// Issuer. Optional.
    pub(crate) iss: String,
    /// Not before (as UTC timestamp). Optional.
    pub(crate) nbf: u64,
    /// Subject (whom token refers to). Optional.
    pub(crate) sub: String,
}

impl Claims {
    /// Create a new Claims instance
    pub fn new(exp: u64) -> Self {
        Self {
            jti: Ulid::new(),
            aud: String::new(),
            exp,
            iat: 0,
            iss: String::new(),
            nbf: 0,
            sub: String::new(),
        }
    }

    /// Set the audience
    pub fn with_audience(self, aud: String) -> Self {
        Self { aud, ..self }
    }

    /// Set the issued at
    pub fn with_issued_at(self, iat: u64) -> Self {
        Self { iat, ..self }
    }

    /// Set the issuer
    pub fn with_issuer(self, iss: String) -> Self {
        Self { iss, ..self }
    }

    /// Set the not before
    pub fn with_not_before(self, nbf: u64) -> Self {
        Self { nbf, ..self }
    }

    /// Set the subject
    pub fn with_subject(self, sub: String) -> Self {
        Self { sub, ..self }
    }
}
