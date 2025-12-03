use http::Uri;
use nr_auth::ClientID;
use serde::Deserialize;
use std::path::PathBuf;

/// Authorization configuration used by the OpAmp connection to NewRelic.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AuthConfig {
    /// Endpoint to obtain the access token presenting the client id and secret.
    #[serde(with = "http_serde::uri")]
    pub token_url: Uri,
    /// Auth client id associated with the provided key.
    pub client_id: ClientID,
    /// Method to sign the client secret used to retrieve the access token.
    // TODO: this is Optional but a default value is set right after deserializing (we cannot implement Default because
    // it needs a value which needs to be injected). We may want to refactor this and use different types: one for
    // deserializing (with optional provider) and one built after setting up the default (with no-option).
    #[serde(flatten)]
    pub provider: Option<ProviderConfig>,
    /// Number of retries for token retrieval. Default 0.
    #[serde(default)]
    pub retries: u8,
}

/// Supported access token request signers methods
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(tag = "provider")]
pub enum ProviderConfig {
    #[serde(rename = "local")]
    Local(LocalConfig),
}

/// Uses a local private key to sign the access token request.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct LocalConfig {
    /// The absolute file path to the private key.
    /// This field is mandatory.
    pub private_key_path: PathBuf,

    /// The actual content of the private key loaded in memory.
    /// This is optional and defaults to `None` upon initialization.
    pub private_key_value: Option<String>,
}

impl LocalConfig {
    /// Creates a new `LocalConfig` with the specified file path.
    /// The `private_key_value` is initialized as `None`.
    pub fn new_with_path(path: PathBuf) -> Self {
        Self {
            private_key_path: path,
            private_key_value: None,
        }
    }

    /// Builder method to set the in-memory private key value.
    /// if the key content is already known
    pub fn new_with_value(value: String) -> Self {
        Self {
            private_key_path: PathBuf::default(),
            private_key_value: Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::opamp::auth::config::{AuthConfig, LocalConfig, ProviderConfig};
    use http::Uri;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn test_deserialize_default() {
        struct Test {
            content: String,
            expected: AuthConfig,
        }
        impl Test {
            fn run(&self) {
                let config: AuthConfig = serde_yaml::from_str(&self.content).unwrap();
                assert_eq!(self.expected, config);
            }
        }

        let tests: Vec<Test> = vec![
            Test {
                content: String::from(
                    r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake"
                "#,
                ),
                expected: AuthConfig {
                    client_id: "fake".into(),
                    token_url: Uri::from_str("http://fake.com/oauth2/v1/token").unwrap(),
                    provider: None,
                    retries: 0u8,
                },
            },
            Test {
                content: String::from(
                    r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake"
retries: 3
                "#,
                ),
                expected: AuthConfig {
                    client_id: "fake".into(),
                    token_url: Uri::from_str("http://fake.com/oauth2/v1/token").unwrap(),
                    provider: None,
                    retries: 3u8,
                },
            },
            Test {
                content: String::from(
                    r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake_client_id"
provider: "local"
private_key_path: "/tmp/fake.key"
private_key_value: "secret"
                    "#,
                ),
                expected: AuthConfig {
                    client_id: "fake_client_id".into(),
                    token_url: Uri::from_str("http://fake.com/oauth2/v1/token").unwrap(),
                    provider: Some(ProviderConfig::Local(LocalConfig {
                        private_key_path: PathBuf::from("/tmp/fake.key"),
                        private_key_value: Some("secret".to_string()),
                    })),
                    retries: 0u8,
                },
            },
        ];

        tests.iter().for_each(|t| t.run());
    }
}
