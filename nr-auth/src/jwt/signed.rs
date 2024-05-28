use chrono::{DateTime, Utc};

pub struct SignedJwt {
    /// Expiration date
    pub(crate) expiration_date: DateTime<Utc>,
    /// Encoded value
    pub(crate) value: String,
}

impl SignedJwt {
    /// Get the expiration date
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expiration_date
    }

    /// Get the encoded value
    pub fn value(&self) -> &str {
        &self.value
    }
}
