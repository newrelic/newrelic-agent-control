use crate::agent_control::config::K8sConfig;
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

    fn retrieve(&self) -> Result<String, Self::Error> {
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
    use assert_matches::assert_matches;
    use mockall::predicate::*;
    use mockall::*;

    mock! {
        pub SecretsProvider {}

        impl SecretsProvider for SecretsProvider {
            type Error = std::io::Error;

            fn get_secret(&self, secret_path: &str) -> Result<String, std::io::Error>;
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
        let expected_path = "test-ns:my-secret:my-key";

        let mut mock_provider = MockSecretsProvider::new();

        mock_provider
            .expect_get_secret()
            .with(eq(expected_path))
            .times(1)
            .returning(|_| Ok("SUPER_SECRET_TOKEN".to_string()));

        let retriever = K8sSecretRetriever::new(mock_provider, config);

        let result = retriever.retrieve().expect("retrieve should not fail");
        assert_eq!(result, "SUPER_SECRET_TOKEN");
    }

    #[test]
    fn test_retrieve_wraps_provider_error() {
        let config = create_dummy_config();

        let mut mock_provider = MockSecretsProvider::new();

        mock_provider
            .expect_get_secret()
            .with(always())
            .returning(|_| Err(std::io::Error::other("Simulated K8s failure")));

        let retriever = K8sSecretRetriever::new(mock_provider, config);

        let result = retriever.retrieve();

        assert_matches!(result, Err(K8sRetrieverError(s)) => {
            assert!(s.contains("K8s getting secret from k8s") && s.contains("Simulated K8s failure"));
        });
    }
}
