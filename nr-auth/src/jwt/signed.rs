pub struct SignedJwt {
    /// Encoded value
    pub(crate) value: String,
}

impl SignedJwt {
    /// Get the encoded value
    pub fn value(&self) -> &str {
        &self.value
    }
}
