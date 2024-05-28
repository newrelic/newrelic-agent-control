use chrono::{DateTime, Utc};

pub struct SignedJwt {
    /// Expiration date
    pub expiration_date: DateTime<Utc>,
    /// Encoded value
    pub value: String,
}

impl SignedJwt {
    /// Get the expiration date
    fn expires_at(&self) -> DateTime<Utc> {
        self.expiration_date
    }

    /// Get the encoded value
    fn value(&self) -> &str {
        &self.value
    }
}
