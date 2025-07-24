use std::sync::Arc;

use thiserror::Error;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::secrets_provider::SecretsProvider;

#[derive(Debug, Error)]
#[error("resolving k8s secret: {0}")]
pub struct K8sSecretProviderError(String);

/// A secrets provider that retrieves secrets from Kubernetes.
pub struct K8sSecretProvider {
    k8s_client: Arc<SyncK8sClient>,
}

impl K8sSecretProvider {
    pub fn new(k8s_client: Arc<SyncK8sClient>) -> Self {
        K8sSecretProvider { k8s_client }
    }
}

impl SecretsProvider for K8sSecretProvider {
    type Error = K8sSecretProviderError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        let K8sSecretPath {
            namespace,
            name,
            key,
        } = K8sSecretPath::try_from(secret_path)?;

        self.k8s_client
            .get_secret_key(&name, &namespace, &key)
            .map_err(|err| K8sSecretProviderError(format!("getting {secret_path} secret: {err}")))?
            .ok_or_else(|| K8sSecretProviderError(format!("'{secret_path}' secret not found")))
    }
}

/// Represents a Kubernetes secret path in the format `<namespace>:<name>:<key>`.
#[derive(Debug)]
pub struct K8sSecretPath {
    namespace: String,
    name: String,
    key: String,
}

/// Converts a format like <namespace>:<name>:<key> into a [K8sSecretPath].
impl TryFrom<&str> for K8sSecretPath {
    type Error = K8sSecretProviderError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split(':').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(K8sSecretProviderError(format!(
                "secret path '{value}' does not have a valid format '<namespace>:<name>:<key>'"
            )));
        }
        Ok(K8sSecretPath {
            namespace: parts[0].to_string(),
            name: parts[1].to_string(),
            key: parts[2].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("ns:name:key", "ns", "name", "key")]
    #[case("-:-:-", "-", "-", "-")]
    fn test_valid_secret_paths(
        #[case] input: &str,
        #[case] expected_namespace: &str,
        #[case] expected_name: &str,
        #[case] expected_key: &str,
    ) {
        let result = K8sSecretPath::try_from(input).unwrap();

        assert_eq!(result.namespace, expected_namespace);
        assert_eq!(result.name, expected_name);
        assert_eq!(result.key, expected_key);
    }

    #[rstest]
    #[case("missingparameter")]
    #[case("missing:parameter")]
    #[case("more:than:three:parameters")]
    #[case("::")]
    #[case("ns:name:")]
    #[case("ns::key")]
    #[case(":name:key")]
    #[case("")]
    fn test_invalid_secret_paths(#[case] input: &str) {
        let result = K8sSecretPath::try_from(input);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("does not have a valid format")
        );
    }
}
