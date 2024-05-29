use serde::{Deserialize, Serialize};
use ulid::Ulid;
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Issuer. Client ID will be used here.
    pub(crate) iss: String,
    /// Subject (whom token refers to). Client ID will be used here.
    pub(crate) sub: String,
    /// Audience. Full URL to the token generation endpoint.
    pub(crate) aud: String,
    /// JWT ID. Must not be re-used. Using ULID.
    pub(crate) jti: Ulid,
    /// Expiration time (as UTC timestamp).
    pub(crate) exp: u64,
}

impl Claims {
    /// Create a new Claims instance
    pub fn new(client_id: String, aud: Url, exp: u64) -> Self {
        Self {
            iss: client_id.clone(),
            sub: client_id,
            aud: aud.to_string(),
            jti: Ulid::new(), // Non-reusable JWT ID
            exp,
        }
    }
}
