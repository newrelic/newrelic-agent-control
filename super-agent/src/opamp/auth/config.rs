use std::path::PathBuf;

use nr_auth::ClientID;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::super_agent::defaults::{AUTH_PRIVATE_KEY_FILE_NAME, SUPER_AGENT_LOCAL_DATA_DIR};

/// Authorization configuration used by the OpAmp connection to NewRelic.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct AuthConfig {
    /// Endpoint to obtain the access token presenting the client id and secret.
    pub token_url: Url,
    /// Auth client id associated with the provided key.
    pub client_id: ClientID,
    /// Method to sign the client secret used to retrieve the access token.
    #[serde(flatten, deserialize_with = "deserialize_default_provider")]
    pub provider: ProviderConfig,
    /// Number of retries for token retrieval. Default 0.
    #[serde(default)]
    pub retries: u8,
}

// This is a workaround for a bug on serde not being able to use default on flattened fields.
// https://github.com/serde-rs/serde/issues/1879
fn deserialize_default_provider<'de, D>(deserializer: D) -> Result<ProviderConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let provider_config = Option::<ProviderConfig>::deserialize(deserializer)?;
    Ok(provider_config.unwrap_or(ProviderConfig::default()))
}

/// Supported access token request signers methods
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
#[serde(tag = "provider")]
pub enum ProviderConfig {
    #[serde(rename = "local")]
    Local(LocalConfig),
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::Local(LocalConfig::default())
    }
}

/// Uses a local private key to sign the access token request.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct LocalConfig {
    /// Private key absolute path.
    pub private_key_path: PathBuf,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            private_key_path: PathBuf::from(SUPER_AGENT_LOCAL_DATA_DIR())
                .join(AUTH_PRIVATE_KEY_FILE_NAME()),
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
                    provider: ProviderConfig::Local(LocalConfig {
                        private_key_path: PathBuf::from("path/to/key"),
                    }),
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
                    provider: ProviderConfig::default(),
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
                    provider: ProviderConfig::default(),
                    retries: 3u8,
                },
            },
        ];

        tests.iter().for_each(|t| t.run());
    }
}
