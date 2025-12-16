use crate::agent_control::config::{K8sConfig, OpAMPClientConfig};
use crate::secret_retriever::OpampSecretRetriever;
use crate::secrets_provider::SecretsProvider;
use crate::secrets_provider::k8s_secret::K8sSecretProvider;

pub struct K8sSecretRetriever<P> {
    provider: P,
    config: K8sConfig,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct K8sRetrieverError(String);

impl<P> K8sSecretRetriever<P>
where
    P: SecretsProvider,
{
    pub fn new(provider: P, config: K8sConfig) -> Self {
        Self { provider, config }
    }
}

impl<P> OpampSecretRetriever for K8sSecretRetriever<P>
where
    P: SecretsProvider,
{
    type Error = K8sRetrieverError;

    fn retrieve(&self, _opamp_config: &OpAMPClientConfig) -> Result<String, Self::Error> {
        let secret_path = K8sSecretProvider::build_secret_path(
            &self.config.namespace,
            &self.config.auth_secret.secret_name,
            &self.config.auth_secret.secret_key_name,
        );

        self.provider.get_secret(&secret_path).map_err(|e| {
            K8sRetrieverError(format!(
                "K8s getting secret from k8s: secret: {secret_path}, error: {e}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::AuthSecret;
    use std::fmt;

    struct MockProvider {
        expected_path: String,
        should_fail: bool,
        return_value: String,
    }

    #[derive(Debug)]
    struct MockError(String);
    impl fmt::Display for MockError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for MockError {}

    impl SecretsProvider for MockProvider {
        type Error = MockError;

        fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
            assert_eq!(
                secret_path, self.expected_path,
                "The retriever constructed the wrong secret path!"
            );

            if self.should_fail {
                Err(MockError("Simulated K8s failure".to_string()))
            } else {
                Ok(self.return_value.clone())
            }
        }
    }

    fn create_dummy_config() -> K8sConfig {
        K8sConfig {
            namespace: "test-ns".to_string(),
            auth_secret: AuthSecret {
                secret_name: "my-secret".to_string(),
                secret_key_name: "my-key".to_string(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_retrieve_success_constructs_correct_path() {
        let config = create_dummy_config();

        let expected_path = "test-ns:my-secret:my-key".to_string();

        let mock_provider = MockProvider {
            expected_path,
            should_fail: false,
            return_value: "SUPER_SECRET_TOKEN".to_string(),
        };

        let retriever = K8sSecretRetriever::new(mock_provider, config);

        let opamp_config = OpAMPClientConfig::default();

        let result = retriever.retrieve(&opamp_config);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "SUPER_SECRET_TOKEN");
    }

    #[test]
    fn test_retrieve_wraps_provider_error() {
        let config = create_dummy_config();
        let expected_path = "test-ns:my-secret:my-key".to_string();

        let mock_provider = MockProvider {
            expected_path,
            should_fail: true,
            return_value: "".to_string(),
        };

        let retriever = K8sSecretRetriever::new(mock_provider, config);
        let opamp_config = OpAMPClientConfig::default();

        let result = retriever.retrieve(&opamp_config);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();

        assert!(err_msg.contains("K8s getting secret from k8s"));
        assert!(err_msg.contains("Simulated K8s failure"));
    }
}
