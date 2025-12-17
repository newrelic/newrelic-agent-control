use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::defaults::AUTH_PRIVATE_KEY_FILE_NAME;
use crate::agent_control::run::BasePaths;
use crate::opamp::auth::config::ProviderConfig;
use crate::secret_retriever::OpampSecretRetriever;
use crate::secrets_provider::SecretsProvider;

/// Helper struct to determine the path and retrieve the secret using the File provider.
pub struct OnHostSecretRetriever<P> {
    opamp_config: Option<OpAMPClientConfig>,
    pub base_paths: BasePaths,
    pub provider: P,
}
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct OnHostRetrieverError(String);
impl<P> OnHostSecretRetriever<P>
where
    P: SecretsProvider,
{
    pub fn new(
        opamp_config: Option<OpAMPClientConfig>,
        base_paths: BasePaths,
        provider: P,
    ) -> Self {
        Self {
            opamp_config,
            base_paths,
            provider,
        }
    }
}

impl<P> OpampSecretRetriever for OnHostSecretRetriever<P>
where
    P: SecretsProvider,
{
    type Error = OnHostRetrieverError;

    fn retrieve(&self) -> Result<String, Self::Error> {
        let mut final_path = self.base_paths.local_dir.join(AUTH_PRIVATE_KEY_FILE_NAME);

        if let Some(opamp_config) = &self.opamp_config
            && let Some(auth_config) = &opamp_config.auth_config
            && let Some(ProviderConfig::Local(local_config)) = &auth_config.provider
        {
            final_path = local_config.private_key_path.clone();
        }

        let secret_path = final_path.to_string_lossy().to_string();

        self.provider
            .get_secret(&secret_path)
            .map_err(|e| OnHostRetrieverError(format!("Failed to retrieve file secret: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::auth::config::{AuthConfig, LocalConfig};
    use http::Uri;
    use nr_auth::ClientID;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn parse_auth_config(yaml: &str) -> AuthConfig {
        serde_yaml::from_str(yaml).expect("Should be able to deserialize the YAML")
    }

    #[test]
    fn test_deserialize_local_provider_config() {
        let yaml = r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake-client-id"
provider: "local"
private_key_path: "/etc/secrets/key.pem"
        "#;

        let config = parse_auth_config(yaml);

        assert_eq!(config.client_id, ClientID::from("fake-client-id"));
        assert_eq!(
            config.token_url,
            Uri::from_str("http://fake.com/oauth2/v1/token").unwrap()
        );
        assert_eq!(config.retries, 0);

        match config.provider {
            Some(ProviderConfig::Local(local_config)) => {
                assert_eq!(
                    local_config.private_key_path,
                    PathBuf::from("/etc/secrets/key.pem")
                );
            }
            _ => panic!("Se esperaba ProviderConfig::Local"),
        }
    }

    #[test]
    fn test_deserialize_no_provider_defaults() {
        let yaml = r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake-client-id"
    "#;

        let config = parse_auth_config(yaml);

        assert_eq!(
            config.token_url,
            Uri::from_str("http://fake.com/oauth2/v1/token").unwrap()
        );
        assert!(
            config.provider.is_none(),
            "Provider should be None if not specified"
        );
        assert_eq!(config.retries, 0, "Default retries should be 0");
    }

    #[test]
    fn test_deserialize_with_retries() {
        let yaml = r#"
token_url: "http://fake.com/oauth2/v1/token"
client_id: "fake-client-id"
retries: 5
        "#;

        let config = parse_auth_config(yaml);

        assert_eq!(config.retries, 5);
        assert!(config.provider.is_none());
    }

    #[test]
    fn test_local_config_constructor() {
        let base_path = PathBuf::from("/var/lib/newrelic");
        let config = LocalConfig::new(base_path.clone());

        let expected_path = base_path.join(AUTH_PRIVATE_KEY_FILE_NAME);
        assert_eq!(config.private_key_path, expected_path);
    }
}
