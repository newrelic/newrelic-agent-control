//! This module contains functions to deal with the health-check of a list of items.
//!
use super::health_checker::LABEL_RELEASE_FLUX;
use crate::{
    k8s::utils::contains_label_with_value,
    sub_agent::health::health_checker::{Health, HealthCheckerError, Healthy},
};
use k8s_openapi::{
    apimachinery::pkg::apis::meta::v1::ObjectMeta, Metadata, NamespaceResourceScope, Resource,
};
use std::{any::Any, sync::Arc};

/// Executes the provided health-check function over the items provided.
/// It returns:
/// * A healthy result if the result of execution is healthy for all the items.
/// * The first encountered error or unhealthy result, otherwise.
pub fn check_health_for_items<K, F>(
    items: impl Iterator<Item = K>,
    health_check_fn: F,
) -> Result<Health, HealthCheckerError>
where
    K: Any,
    F: Fn(K) -> Result<Health, HealthCheckerError>,
{
    for obj in items {
        let obj_health = health_check_fn(obj)?;
        if !obj_health.is_healthy() {
            return Ok(obj_health);
        }
    }
    Ok(Healthy::default().into())
}

/// Returns a closure which can be used as filter predicate. It will filter objects labeled with the key
/// [LABEL_RELEASE_FLUX] and the provided release name as value.
pub fn flux_release_filter<K>(release_name: String) -> impl Fn(&Arc<K>) -> bool
where
    K: Metadata<Ty = ObjectMeta>,
{
    // TODO: when https://github.com/kube-rs/kube/pull/1482, is ready, a label-selector could be used instead.
    move |obj| {
        contains_label_with_value(
            &obj.metadata().labels,
            LABEL_RELEASE_FLUX,
            release_name.as_str(),
        )
    }
}

/// Return the value of `.metadata.name` of the object that is passed.
pub fn get_metadata_name<K>(obj: &K) -> Result<String, HealthCheckerError>
where
    K: Resource<Scope = NamespaceResourceScope> + Metadata<Ty = ObjectMeta>,
{
    let metadata = obj.metadata();

    metadata.name.clone().ok_or_else(|| {
        HealthCheckerError::K8sError(crate::k8s::error::K8sError::MissingName(
            K::KIND.to_string(),
        ))
    })
}

/// Return Kind of given object.
pub fn get_kind<K>(_: &K) -> &str
where
    K: Resource<Scope = NamespaceResourceScope>,
{
    K::KIND
}

/// Helper to return an error when an expected field in the StatefulSet object is missing.
pub fn missing_field_error<K>(obj: &K, name: &str, field: &str) -> HealthCheckerError
where
    K: Resource<Scope = NamespaceResourceScope>,
{
    HealthCheckerError::MissingK8sObjectField {
        kind: get_kind(obj).to_string(),
        name: name.to_string(),
        field: field.to_string(),
    }
}

/// Helper to return an healthy status.
pub fn healthy(s: String) -> Health {
    Healthy { status: s }.into()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{k8s::Error, sub_agent::health::health_checker::Unhealthy};
    use assert_matches::assert_matches;
    use k8s_openapi::api::core::v1::Pod;

    #[test]
    fn test_items_health_check_healthy() {
        let result = check_health_for_items(vec!["a", "b", "c", "d"].into_iter(), |_| {
            Ok(Healthy::default().into())
        })
        .unwrap_or_else(|err| panic!("unexpected error {err} when all items are healthy"));
        assert_eq!(
            result,
            Health::Healthy(Healthy::default()),
            "Expected healthy when all items are healthy"
        );

        let result = check_health_for_items(Vec::new().into_iter(), |_: &str| {
            Err(HealthCheckerError::Generic("fail!".to_string()))
        })
        .unwrap_or_else(|err| panic!("unexpected error {err} when there are no items"));
        assert_eq!(
            result,
            Health::Healthy(Healthy::default()),
            "expected healthy when there are no items"
        );
    }

    #[test]
    fn test_items_health_check_unhealthy() {
        let result = check_health_for_items(vec!["a", "b", "c", "d"].into_iter(), |s| match s {
            "a" | "b" => Ok(Healthy::default().into()),
            _ => Ok(Health::unhealthy_with_last_error(s.to_string())),
        })
        .unwrap_or_else(|err| panic!("unexpected error {err} when unhealthy is expected"));
        assert_eq!(
            result,
            Health::Unhealthy(Unhealthy {
                last_error: "c".to_string(),
                ..Default::default()
            }),
            "expected the first unhealthy found",
        );
    }

    #[test]
    fn test_items_health_check_err() {
        let result = check_health_for_items(vec!["a", "b", "c", "d"].into_iter(), |s| match s {
            "a" | "b" => Ok(Healthy::default().into()),
            _ => Err(HealthCheckerError::Generic(s.to_string())),
        })
        .unwrap_err();
        let s = assert_matches!(result, HealthCheckerError::Generic(s) => s);
        assert_eq!(s, "c", "expected the first error found");
    }

    #[test]
    fn test_flux_release_filter() {
        let release_name = "release-name";
        let objs = ["a+", "b-", "c+", "d-"].map(|s| {
            let mut pod = Pod {
                metadata: ObjectMeta {
                    name: Some(s.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            };
            if s.ends_with('+') {
                // pods whose name ends with + get the release label
                pod.metadata.labels =
                    Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into())
            }
            Arc::new(pod)
        });

        let filtered = objs
            .into_iter()
            .filter(flux_release_filter(release_name.to_string()));

        assert_eq!(
            vec!["a+".to_string(), "c+".to_string()],
            filtered
                .map(|pod| pod.metadata.name.as_ref().unwrap().clone())
                .collect::<Vec<String>>()
        );
    }

    #[test]
    fn test_metadata_name() {
        // As it is a generic, I want to test with at least two different types.
        // Let's start with a Deployment
        let mut deployment = k8s_openapi::api::apps::v1::Deployment {
            ..Default::default()
        };
        let deployment_error = get_metadata_name(&deployment).unwrap_err();
        deployment.metadata.name = Some("name".into());
        let deployment_name = get_metadata_name(&deployment).unwrap();
        assert_eq!(
            deployment_error.to_string(),
            HealthCheckerError::K8sError(Error::MissingName("Deployment".to_string())).to_string()
        );
        assert_eq!(deployment_name, "name".to_string());

        // Now a DaemonSet
        let mut daemon_set = k8s_openapi::api::apps::v1::DaemonSet {
            ..Default::default()
        };
        let daemon_set_error = get_metadata_name(&daemon_set).unwrap_err();
        daemon_set.metadata.name = Some("name".into());
        let daemon_set_name = get_metadata_name(&daemon_set).unwrap();
        assert_eq!(
            daemon_set_error.to_string(),
            HealthCheckerError::K8sError(Error::MissingName("DaemonSet".to_string())).to_string()
        );
        assert_eq!(daemon_set_name, "name".to_string());
    }

    #[test]
    fn test_kind() {
        // As it is a generic, I want to test with at least two different types.
        // Let's start with a Deployment
        let deployment = k8s_openapi::api::apps::v1::Deployment {
            ..Default::default()
        };
        assert_eq!(get_kind(&deployment), "Deployment");

        // Now a DaemonSet
        let daemon_set = k8s_openapi::api::apps::v1::DaemonSet {
            ..Default::default()
        };
        assert_eq!(get_kind(&daemon_set), "DaemonSet");
    }
}
