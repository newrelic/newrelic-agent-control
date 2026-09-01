//! Value provider that reads values from Kubernetes ConfigMaps.

use std::sync::Arc;

use thiserror::Error;

use crate::k8s::client::{K8sClient, SyncK8sClient};
use crate::value_provider::ValueProvider;

/// Error returned when a Kubernetes ConfigMap value cannot be resolved.
#[derive(Debug, Error)]
#[error("resolving k8s configmap: {0}")]
pub struct K8sConfigMapProviderError(String);

/// A value provider that retrieves values from Kubernetes ConfigMaps.
pub struct K8sConfigMapProvider<C: K8sClient = SyncK8sClient> {
    k8s_client: Arc<C>,
}

impl<C: K8sClient> K8sConfigMapProvider<C> {
    /// Creates a new [`K8sConfigMapProvider`] backed by the given Kubernetes client.
    pub fn new(k8s_client: Arc<C>) -> Self {
        K8sConfigMapProvider { k8s_client }
    }
}

impl<C: K8sClient> ValueProvider for K8sConfigMapProvider<C> {
    type Error = K8sConfigMapProviderError;

    fn get_value(&self, configmap_path: &str) -> Result<String, Self::Error> {
        let K8sConfigMapPath {
            namespace,
            name,
            key,
        } = K8sConfigMapPath::try_from(configmap_path)?;

        self.k8s_client
            .get_configmap_key(&name, &namespace, &key)
            .map_err(|err| {
                K8sConfigMapProviderError(format!("getting {configmap_path} configmap: {err}"))
            })?
            .ok_or_else(|| {
                K8sConfigMapProviderError(format!("'{configmap_path}' configmap not found"))
            })
    }
}

/// Represents a Kubernetes ConfigMap path in the format `<namespace>:<name>:<key>`.
#[derive(Debug)]
pub struct K8sConfigMapPath {
    namespace: String,
    name: String,
    key: String,
}

/// Converts a format like `<namespace>:<name>:<key>` into a [K8sConfigMapPath].
impl TryFrom<&str> for K8sConfigMapPath {
    type Error = K8sConfigMapProviderError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split(':').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(K8sConfigMapProviderError(format!(
                "configmap path '{value}' does not have a valid format '<namespace>:<name>:<key>'"
            )));
        }
        Ok(K8sConfigMapPath {
            namespace: parts[0].to_string(),
            name: parts[1].to_string(),
            key: parts[2].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::k8s::client::tests::MockK8sClient;
    use crate::k8s::error::K8sError;
    use assert_matches::assert_matches;
    use rstest::rstest;

    #[rstest]
    #[case("ns:name:key", "ns", "name", "key")]
    #[case("-:-:-", "-", "-", "-")]
    fn test_valid_configmap_paths(
        #[case] input: &str,
        #[case] expected_namespace: &str,
        #[case] expected_name: &str,
        #[case] expected_key: &str,
    ) {
        let result = K8sConfigMapPath::try_from(input).unwrap();

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
    fn test_invalid_configmap_paths(#[case] input: &str) {
        let result = K8sConfigMapPath::try_from(input);
        assert_matches!(result, Err(K8sConfigMapProviderError(msg)) => {
            assert!(msg.contains("does not have a valid format"))
        });
    }

    #[test]
    fn test_get_value_success() {
        let mut k8s_client = MockK8sClient::new();
        k8s_client
            .expect_get_configmap_key()
            .withf(|name, namespace, key| name == "name" && namespace == "ns" && key == "key")
            .returning(|_, _, _| Ok(Some("value".to_string())));

        let provider = K8sConfigMapProvider::new(Arc::new(k8s_client));

        let result = provider.get_value("ns:name:key").unwrap();

        assert_eq!(result, "value");
    }

    #[test]
    fn test_get_value_not_found() {
        let mut k8s_client = MockK8sClient::new();
        k8s_client
            .expect_get_configmap_key()
            .returning(|_, _, _| Ok(None));

        let provider = K8sConfigMapProvider::new(Arc::new(k8s_client));

        let result = provider.get_value("ns:name:key");
        assert_matches!(result, Err(K8sConfigMapProviderError(msg)) => {
            assert!(msg.contains("not found"));
        });
    }

    #[test]
    fn test_get_value_client_error() {
        let mut k8s_client = MockK8sClient::new();
        k8s_client
            .expect_get_configmap_key()
            .returning(|_, _, _| Err(K8sError::Generic("boom".to_string())));

        let provider = K8sConfigMapProvider::new(Arc::new(k8s_client));

        let result = provider.get_value("ns:name:key");

        assert_matches!(result, Err(K8sConfigMapProviderError(msg)) => {
            assert!(msg.contains("boom"));
        });
    }

    #[test]
    fn test_get_value_invalid_path() {
        let k8s_client = MockK8sClient::new();

        let provider = K8sConfigMapProvider::new(Arc::new(k8s_client));

        let result = provider.get_value("invalid");

        assert_matches!(result, Err(K8sConfigMapProviderError(msg)) => {
            assert!(msg.contains("does not have a valid format"));
        });
    }
}
