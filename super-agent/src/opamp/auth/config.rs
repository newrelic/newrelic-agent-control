use std::path::PathBuf;

use nr_auth::ClientID;
use serde::Deserialize;
use url::Url;

use crate::super_agent::defaults::AUTH_PRIVATE_KEY_FILE_NAME;

/// Authorization configuration used by the OpAmp connection to NewRelic.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AuthConfig {
    /// Endpoint to obtain the access token presenting the client id and secret.
    pub token_url: Url,
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
    /// Private key absolute path.
    pub private_key_path: PathBuf,
}

impl LocalConfig {
    pub fn new(local_data_dir: &str) -> Self {
        Self {
            private_key_path: PathBuf::from(local_data_dir).join(AUTH_PRIVATE_KEY_FILE_NAME()),
        }
    }
}

#[cfg(test)]
mod test {
    use std::{path::PathBuf, str::FromStr};

    use url::Url;

    use crate::opamp::auth::config::{AuthConfig, LocalConfig, ProviderConfig};

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
provider: "local"
private_key_path: "path/to/key"
                "#,
                ),
                expected: AuthConfig {
                    client_id: "fake".into(),
                    token_url: Url::from_str("http://fake.com/oauth2/v1/token").unwrap(),
                    provider: Some(ProviderConfig::Local(LocalConfig {
                        private_key_path: PathBuf::from("path/to/key"),
                    })),
                    retries: 0u8,
                },
            },
            Test {
                content: String::from(
                    r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake"
                "#,
                ),
                expected: AuthConfig {
                    client_id: "fake".into(),
                    token_url: Url::from_str("http://fake.com/oauth2/v1/token").unwrap(),
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
                    token_url: Url::from_str("http://fake.com/oauth2/v1/token").unwrap(),
                    provider: None,
                    retries: 3u8,
                },
            },
        ];

        tests.iter().for_each(|t| t.run());
    }
}
