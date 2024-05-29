use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{api::DynamicObject, core::GroupVersion, Api, Client};
use std::{error::Error, str::FromStr};

/// Checks for the existence of specified deployments within a namespace.
pub async fn check_deployments_exist(
    k8s_client: Client,
    names: &[&str],
    namespace: &str,
) -> Result<(), Box<dyn Error>> {
    let api: Api<Deployment> = Api::namespaced(k8s_client.clone(), namespace);

    for &name in names {
        let _ = api
            .get(name)
            .await
            .map_err(|err| format!("Deployment {name} not found: {err}"))?;
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
    expected_valus_as_yaml: &str,
) -> Result<(), Box<dyn Error>> {
    let expected_as_json: serde_json::Value = serde_yaml::from_str(expected_valus_as_yaml).unwrap();
    let gvk = &GroupVersion::from_str("helm.toolkit.fluxcd.io/v2beta2")
        .unwrap()
        .with_kind("HelmRelease");
    let (api_resource, _) = kube::discovery::pinned_kind(&k8s_client, gvk).await?;
    let api: Api<DynamicObject> =
        Api::namespaced_with(k8s_client.clone(), namespace, &api_resource);

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
