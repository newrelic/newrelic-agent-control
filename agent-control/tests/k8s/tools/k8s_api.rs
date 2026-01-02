use crate::common::runtime::block_on;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::api::{DeleteParams, PostParams};
use kube::{Api, Client, api::DynamicObject, core::GroupVersion};
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use std::collections::BTreeMap;
use std::time::Duration;
use std::{error::Error, str::FromStr};
use tokio::time::sleep;

/// Checks for the existence of specified deployments within a namespace.
pub fn check_deployments_exist(
    k8s_client: Client,
    names: &[&str],
    namespace: &str,
) -> Result<(), Box<dyn Error>> {
    let api: Api<Deployment> = Api::namespaced(k8s_client.clone(), namespace);

    for &name in names {
        let _ =
            block_on(api.get(name)).map_err(|err| format!("Deployment {name} not found: {err}"))?;
    }
    Ok(())
}

pub async fn check_config_map_exist(
    k8s_client: Client,
    name: &str,
    namespace: &str,
) -> Result<(), Box<dyn Error>> {
    let api: Api<ConfigMap> = Api::namespaced(k8s_client.clone(), namespace);

    api.get(name)
        .await
        .map_err(|err| format!("ConfigMap {name} not found: {err}"))?;

    Ok(())
}

/// Check if the `HelmRelease` with the provided name has the the expected value in the `spec.values` field.
pub async fn check_helmrelease_spec_values(
    k8s_client: Client,
    namespace: &str,
    name: &str,
    expected_values_as_yaml: &str,
) -> Result<(), Box<dyn Error>> {
    let expected_as_json: serde_json::Value =
        serde_yaml::from_str(expected_values_as_yaml).unwrap();
    let api = helmrelease_api(k8s_client, namespace).await;

    let obj = api.get(name).await?;
    let found_values = &obj.data["spec"]["values"];
    if expected_as_json != *found_values {
        return Err(format!(
            "helm release spec values don't match with expected. Expected: {:?}, Found: {:?}",
            expected_as_json, *found_values,
        )
        .into());
    }
    Ok(())
}

/// Check if the `HelmRelease` with the provided name contains the expected labels.
pub async fn check_helmrelease_labels_contains(
    k8s_client: Client,
    namespace: &str,
    name: &str,
    expected_labels: Option<BTreeMap<String, String>>,
) -> Result<(), Box<dyn Error>> {
    let api = helmrelease_api(k8s_client, namespace).await;

    let obj = api.get(name).await?;
    let found_labels = &obj.metadata.labels;
    if found_labels.is_none() && expected_labels.is_some() {
        return Err(format!(
            "helm release spec labels are None, but expected: {expected_labels:?}"
        )
        .into());
    }

    let found_labels = found_labels.as_ref().unwrap();
    let expected_labels = expected_labels.unwrap_or_default();
    for (key, value) in &expected_labels {
        if found_labels.get(key) != Some(value) {
            return Err(format!(
                "helm release spec labels don't match with expected. Expected: {expected_labels:?}, Found: {found_labels:?}",
            )
            .into());
        }
    }
    Ok(())
}

/// Check if the `HelmRelease` exists.
pub async fn check_helmrelease_exists(
    k8s_client: Client,
    namespace: &str,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let api = helmrelease_api(k8s_client, namespace).await;
    api.get(name).await?;
    Ok(())
}

/// Check if the `HelmRelease` chart version.
pub async fn check_helmrelease_chart_version(
    k8s_client: Client,
    namespace: &str,
    name: &str,
    version: &str,
) -> Result<(), Box<dyn Error>> {
    let api = helmrelease_api(k8s_client, namespace).await;
    let hr = api.get(name).await?;
    let retrieved_version = hr.data["spec"]["chart"]["spec"]["version"].clone();

    if retrieved_version != version {
        return Err(format!(
            "HelmRelease chart version mismatch. Expected: {version}, Found: {retrieved_version}",
        )
        .into());
    }
    Ok(())
}

/// Delete the helm release with "name" and from "namespace"
pub async fn delete_helm_release(
    k8s_client: Client,
    namespace: &str,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let api = helmrelease_api(k8s_client, namespace).await;
    if api.delete(name, &Default::default()).await?.is_left() {
        // left signals that object is being deleted, waiting some time to ensure it is deleted.
        sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}

/// Create the k8s api to be used by other functions
async fn helmrelease_api(k8s_client: Client, namespace: &str) -> Api<DynamicObject> {
    let gvk = &GroupVersion::from_str(helmrelease_v2_type_meta().api_version.as_str())
        .unwrap()
        .with_kind(helmrelease_v2_type_meta().kind.as_str());
    let (api_resource, _) = kube::discovery::pinned_kind(&k8s_client, gvk)
        .await
        .unwrap();

    Api::namespaced_with(k8s_client.clone(), namespace, &api_resource)
}

/// This helper creates a values secret with the provided `secret_name`, `values_key` and `values`.
pub fn create_values_secret(
    k8s_client: Client,
    namespace: &str,
    secret_name: &str,
    values_key: &str,
    values: String,
) {
    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        string_data: Some(BTreeMap::from([(values_key.to_string(), values)])),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(k8s_client, namespace);
    let _ = block_on(secrets.delete(secret_name, &DeleteParams::default()));
    block_on(secrets.create(&PostParams::default(), &secret)).unwrap();
}
