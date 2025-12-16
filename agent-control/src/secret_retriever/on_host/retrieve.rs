use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::defaults::AUTH_PRIVATE_KEY_FILE_NAME;
use crate::agent_control::run::BasePaths;
use crate::opamp::auth::config::ProviderConfig;
use crate::secret_retriever::OpampSecretRetriever;
use crate::secrets_provider::SecretsProvider;

/// Helper struct to determine the path and retrieve the secret using the File provider.
pub struct OnHostSecretRetriever<P> {
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
    pub fn new(base_paths: BasePaths, provider: P) -> Self {
        Self {
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

    fn retrieve(&self, opamp_config: &OpAMPClientConfig) -> Result<String, Self::Error> {
        let path_buf = if let Some(auth_config) = &opamp_config.auth_config {
            if let Some(ProviderConfig::Local(local_config)) = &auth_config.provider {
                local_config.private_key_path.clone()
            } else {
                self.base_paths.local_dir.join(AUTH_PRIVATE_KEY_FILE_NAME)
            }
        } else {
            self.base_paths.local_dir.join(AUTH_PRIVATE_KEY_FILE_NAME)
        };

        let secret_path = path_buf.to_string_lossy().to_string();

        self.provider
            .get_secret(&secret_path)
            .map_err(|e| OnHostRetrieverError(format!("Failed to retrieve file secret: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::auth::config::{AuthConfig, LocalConfig};
    use crate::opamp::client_builder::PollInterval;
    use http::HeaderMap;
    use std::fmt;
    use std::path::PathBuf;
    use url::Url;

    struct MockProvider {
        expected_path_tocheck: String,
        should_succeed: bool,
    }

    #[derive(Debug)]
    struct MockError;
    impl fmt::Display for MockError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "MockError")
        }
    }
    impl std::error::Error for MockError {}

    impl SecretsProvider for MockProvider {
        type Error = MockError;

        fn get_secret(&self, path: &str) -> Result<String, Self::Error> {
            assert_eq!(
                path, self.expected_path_tocheck,
                "The retriever passed the wrong path to the provider"
            );

            if self.should_succeed {
                Ok("SECRET_CONTENT".to_string())
            } else {
                Err(MockError)
            }
        }
    }

    fn get_base_paths() -> BasePaths {
        BasePaths {
            local_dir: PathBuf::from("/etc/newrelic"),
            remote_dir: PathBuf::from("/tmp"),
            log_dir: PathBuf::from("/var/log"),
        }
    }

    fn create_opamp_config(provider: Option<ProviderConfig>) -> OpAMPClientConfig {
        OpAMPClientConfig {
            endpoint: Url::parse("http://localhost:8080").unwrap(),

            poll_interval: PollInterval::default(),
            headers: HeaderMap::default(),
            fleet_id: "test-fleet-id".to_string(),
            signature_validation: Default::default(),

            auth_config: Some(AuthConfig {
                provider,
                client_id: "dummy-client".to_string(),
                token_url: "http://dummy".parse::<http::Uri>().unwrap(),
                retries: 3,
            }),
        }
    }

    #[test]
    fn test_retrieve_uses_explicit_config_path() {
        let custom_path = "/custom/keys/my_key.pem";

        let local_config = LocalConfig {
            private_key_path: PathBuf::from(custom_path),
            private_key_value: None,
        };

        let config = create_opamp_config(Some(ProviderConfig::Local(local_config)));

        let mock = MockProvider {
            expected_path_tocheck: custom_path.to_string(),
            should_succeed: true,
        };

        let retriever = OnHostSecretRetriever::new(get_base_paths(), mock);
        let result = retriever.retrieve(&config);

        assert_eq!(result.unwrap(), "SECRET_CONTENT");
    }

    #[test]
    fn test_retrieve_uses_default_path_when_provider_is_none() {
        let config = create_opamp_config(None);

        let expected_default = PathBuf::from("/etc/newrelic").join(AUTH_PRIVATE_KEY_FILE_NAME);

        let mock = MockProvider {
            expected_path_tocheck: expected_default.to_string_lossy().to_string(),
            should_succeed: true,
        };

        let retriever = OnHostSecretRetriever::new(get_base_paths(), mock);
        let result = retriever.retrieve(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_retrieve_uses_default_path_when_no_auth_config() {
        let mut config = create_opamp_config(None);
        config.auth_config = None;

        let expected_default = PathBuf::from("/etc/newrelic").join(AUTH_PRIVATE_KEY_FILE_NAME);

        let mock = MockProvider {
            expected_path_tocheck: expected_default.to_string_lossy().to_string(),
            should_succeed: true,
        };

        let retriever = OnHostSecretRetriever::new(get_base_paths(), mock);
        let result = retriever.retrieve(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_retrieve_wraps_provider_errors() {
        let config = create_opamp_config(None);
        let expected_default = PathBuf::from("/etc/newrelic").join(AUTH_PRIVATE_KEY_FILE_NAME);

        let mock = MockProvider {
            expected_path_tocheck: expected_default.to_string_lossy().to_string(),
            should_succeed: false,
        };

        let retriever = OnHostSecretRetriever::new(get_base_paths(), mock);
        let result = retriever.retrieve(&config);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();

        assert!(err_msg.contains("Failed to retrieve file secret"));
        assert!(err_msg.contains("MockError"));
    }
}
