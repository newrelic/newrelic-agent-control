use std::path::PathBuf;

use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::defaults::AUTH_PRIVATE_KEY_FILE_NAME;
use crate::opamp::auth::config::ProviderConfig;
use crate::secret_retriever::OpampSecretRetriever;
use crate::secrets_provider::SecretsProvider;

/// Helper struct to determine the path and retrieve the secret using the File provider.
pub struct OnHostSecretRetriever<P> {
    opamp_config: Option<OpAMPClientConfig>,
    pub local_dir: PathBuf,
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
        local_dir: impl Into<PathBuf>,
        provider: P,
    ) -> Self {
        Self {
            opamp_config,
            local_dir: local_dir.into(),
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
        let mut final_path = self.local_dir.join(AUTH_PRIVATE_KEY_FILE_NAME);

        if let Some(opamp_config) = &self.opamp_config
            && let Some(auth_config) = &opamp_config.auth_config
            && let Some(ProviderConfig::Local(local_config)) = &auth_config.provider
        {
            final_path = local_config.private_key_path.clone();
        }

        let secret_path = final_path.to_string_lossy().to_string();

        self.provider
            .get_secret(&secret_path)
            .map_err(|e| OnHostRetrieverError(format!("Failed to retrieve file secret: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::auth::config::{AuthConfig, LocalConfig};
    use crate::secret_retriever::test_mocks::MockSecretsProvider;
    use mockall::predicate::*;
    use std::path::PathBuf;

    const TEST_LOCAL_DIR: &str = "/default/local";

    fn create_dummy_opamp_config(custom_path: Option<&str>) -> OpAMPClientConfig {
        use http::Uri;
        use nr_auth::ClientID;
        use std::str::FromStr;

        let provider = custom_path.map(|p| {
            ProviderConfig::Local(LocalConfig {
                private_key_path: PathBuf::from(p),
            })
        });

        OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            poll_interval: Default::default(),
            headers: Default::default(),
            auth_config: Some(AuthConfig {
                token_url: Uri::from_str("http://localhost").unwrap(),
                client_id: ClientID::from("test"),
                provider,
                retries: 0,
            }),
            fleet_id: "".to_string(),
            signature_validation: Default::default(),
        }
    }

    #[test]
    fn test_retrieve_uses_default_path_when_no_config() {
        let local_dir = PathBuf::from(TEST_LOCAL_DIR);
        let expected_path = local_dir
            .join(AUTH_PRIVATE_KEY_FILE_NAME)
            .to_string_lossy()
            .to_string();

        let mut mock_provider = MockSecretsProvider::new();
        mock_provider
            .expect_get_secret()
            .with(eq(expected_path.clone()))
            .times(1)
            .returning(|_| Ok("SECRET_CONTENT".to_string()));

        let retriever = OnHostSecretRetriever::new(None, local_dir, mock_provider);

        let result = retriever.retrieve();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "SECRET_CONTENT");
    }

    #[test]
    fn test_retrieve_uses_configured_path_when_provided() {
        let local_dir = PathBuf::from(TEST_LOCAL_DIR);
        let custom_path = "/etc/custom/key.pem";

        let opamp_config = create_dummy_opamp_config(Some(custom_path));

        let mut mock_provider = MockSecretsProvider::new();
        mock_provider
            .expect_get_secret()
            .with(eq(custom_path.to_string()))
            .times(1)
            .returning(|_| Ok("CUSTOM_SECRET".to_string()));

        let retriever = OnHostSecretRetriever::new(Some(opamp_config), local_dir, mock_provider);

        let result = retriever.retrieve();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "CUSTOM_SECRET");
    }

    #[test]
    fn test_retrieve_fallback_to_default_if_provider_is_not_local() {
        let local_dir = PathBuf::from(TEST_LOCAL_DIR);
        let expected_default_path = local_dir
            .join(AUTH_PRIVATE_KEY_FILE_NAME)
            .to_string_lossy()
            .to_string();

        let opamp_config = create_dummy_opamp_config(None);

        let mut mock_provider = MockSecretsProvider::new();
        mock_provider
            .expect_get_secret()
            .with(eq(expected_default_path))
            .times(1)
            .returning(|_| Ok("DEFAULT_SECRET".to_string()));

        let retriever = OnHostSecretRetriever::new(Some(opamp_config), local_dir, mock_provider);

        let result = retriever.retrieve();
        assert_eq!(result.unwrap(), "DEFAULT_SECRET");
    }

    #[test]
    fn test_retrieve_handles_provider_errors() {
        let local_dir = PathBuf::from(TEST_LOCAL_DIR);
        let mut mock_provider = MockSecretsProvider::new();

        mock_provider.expect_get_secret().returning(|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "File not found",
            ))
        });

        let retriever = OnHostSecretRetriever::new(None, local_dir, mock_provider);

        let result = retriever.retrieve();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to retrieve file secret")
        );
    }
}
