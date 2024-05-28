use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// JWT ID. Required.
    jti: Ulid,
    /// Audience. Optional.
    aud: String,
    /// Expiration time (as UTC timestamp). Required.
    pub(crate) exp: usize,
    /// Issued at (as UTC timestamp). Optional.
    iat: usize,
    /// Issuer. Optional.
    iss: String,
    /// Not before (as UTC timestamp). Optional.
    nbf: usize,
    /// Subject (whom token refers to). Optional.
    sub: String,
}

impl Claims {
    /// Create a new Claims instance
    pub fn new(exp: usize) -> Self {
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
    pub fn with_issued_at(self, iat: usize) -> Self {
        Self { iat, ..self }
    }

    /// Set the issuer
    pub fn with_issuer(self, iss: String) -> Self {
        Self { iss, ..self }
    }

    /// Set the not before
    pub fn with_not_before(self, nbf: usize) -> Self {
        Self { nbf, ..self }
    }

    /// Set the subject
    pub fn with_subject(self, sub: String) -> Self {
        Self { sub, ..self }
    }
}
