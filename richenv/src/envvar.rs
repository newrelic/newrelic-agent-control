use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;
use thiserror::Error;

// Regex to validate environment variables keys:
// uppercase letters, digits, and the '_' (underscore) from the characters defined in
// Portable Character Set and do not begin with a digit
// https://pubs.opengroup.org/onlinepubs/000095399/basedefs/xbd_chap08.html
const ENV_VAR_KEY_REGEX: &str = r"^[a-zA-Z_][a-zA-Z0-9_]*$";

// build regex to validate env var keys just once
pub(super) fn env_var_key_regex() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(ENV_VAR_KEY_REGEX).unwrap())
}

#[derive(Error, Debug, Clone)]
pub enum EnvVarError {
    #[error("invalid key format")]
    InvalidKeyFormat,
}

#[derive(Default, Debug, Hash, PartialEq, Eq)]
pub struct EnvVarKey(String);
#[derive(Default, Debug)]
pub struct EnvVarValue(String);
#[derive(Default, Debug)]
pub struct EnvVars(HashMap<EnvVarKey, EnvVarValue>);

impl EnvVars {
    pub fn with_var(mut self, key: EnvVarKey, val: EnvVarValue) -> Self {
        self.0.insert(key, val);
        self
    }
}

impl TryFrom<String> for EnvVarKey {
    type Error = EnvVarError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !env_var_key_regex().is_match(value.as_str()) {
            return Err(EnvVarError::InvalidKeyFormat);
        }
        Ok(EnvVarKey(value))
    }
}
