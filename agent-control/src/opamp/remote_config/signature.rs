use opamp_client::opamp::proto::CustomMessage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
pub const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "config_signature";

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct Signature {
    signature: Vec<u8>,
}
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SignatureError {
    #[error("invalid config signature capability")]
    InvalidCapability,
    #[error("invalid config signature type")]
    InvalidType,
    #[error("invalid config signature data")]
    InvalidData(String),
}

impl TryFrom<&CustomMessage> for Signature {
    type Error = SignatureError;

    fn try_from(custom_message: &CustomMessage) -> Result<Self, Self::Error> {
        if custom_message.capability != SIGNATURE_CUSTOM_CAPABILITY {
            return Err(SignatureError::InvalidCapability);
        }
        if custom_message.r#type != SIGNATURE_CUSTOM_MESSAGE_TYPE {
            return Err(SignatureError::InvalidType);
        }
        let signature = serde_json::from_slice(&custom_message.data)
            .map_err(|err| SignatureError::InvalidData(err.to_string()))?;

        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::Signature;

    impl Signature {
        pub fn new(signature: Vec<u8>) -> Self {
            Self { signature }
        }
    }
}
