//! Value provider that reads values from environment variables.

use std::env::VarError;

use crate::value_provider::ValueProvider;

/// A value provider that retrieves values from environment variables.
pub struct Env;

impl ValueProvider for Env {
    type Error = VarError;

    fn get_value(&self, path: &str) -> Result<String, Self::Error> {
        std::env::var(path)
    }
}
