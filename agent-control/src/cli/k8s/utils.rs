use super::errors::K8sCliError;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::TypeMeta;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tracing::debug;

/// Parses a string of key-value pairs separated by commas.
///
/// The equal sign character `=` is used to separate the key from the value,
/// and the comma character `,` to separate the pairs.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
/// use newrelic_agent_control::cli::k8s::utils::parse_key_value_pairs;
///
/// let data = "key1=value1, key2=value2, key3=value3";
/// let parsed = parse_key_value_pairs(data);
/// assert_eq!(parsed, BTreeMap::from([
///     ("key1".to_string(), "value1".to_string()),
///     ("key2".to_string(), "value2".to_string()),
///     ("key3".to_string(), "value3".to_string()),
/// ]));
/// ```
pub fn parse_key_value_pairs(data: impl AsRef<str>) -> BTreeMap<String, String> {
    let pairs = data.as_ref().split(',');
    let key_values = pairs.map(|pair| pair.split_once('='));
    let valid_key_values = key_values.flatten();

    valid_key_values
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

/// Returns a list of all available API resources in the Kubernetes cluster.
pub fn retrieve_api_resources(
    k8s_client: &SyncK8sClient,
) -> Result<HashSet<TypeMeta>, K8sCliError> {
    let mut tm_available = HashSet::new();

    let all_api_resource_list = k8s_client
        .list_api_resources()
        .map_err(|err| K8sCliError::Generic(format!("failed to retrieve api_resources: {err}")))?;

    for api_resource_list in &all_api_resource_list {
        for resource in &api_resource_list.resources {
            tm_available.insert(TypeMeta {
                api_version: api_resource_list.group_version.clone(),
                kind: resource.kind.clone(),
            });
        }
    }

    Ok(tm_available)
}

pub fn try_new_k8s_client() -> Result<SyncK8sClient, K8sCliError> {
    debug!("Starting the runtime");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio should be able to create a runtime"),
    );

    debug!("Starting the k8s client");
    SyncK8sClient::try_new(runtime).map_err(|err| K8sCliError::K8sClient(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;

    #[rstest]
    #[case::valid_data("key1=value1,key2=value2,key3=value3", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::valid_data_with_surrounding_whitespaces(" key1=value1  ,     key2=value2,key3=value3     ", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::data_with_invalid_key_value_pair("key1=value1,key2/value2,key3=value3", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::key_value_pair_with_two_equal_signs("key1=test-value-with=sign", BTreeMap::from([
        ("key1".to_string(), "test-value-with=sign".to_string()),
    ]))]
    #[case::invalid_data("invalid data", BTreeMap::new())]
    #[case::empty("", BTreeMap::new())]
    fn test_parse_key_value_pairs(#[case] data: &str, #[case] expected: BTreeMap<String, String>) {
        assert_eq!(parse_key_value_pairs(data), expected);
    }
}
