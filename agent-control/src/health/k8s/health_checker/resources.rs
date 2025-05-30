//! This module includes implementations of health-checkers for individual k8s resources.
//!
use super::LABEL_RELEASE_FLUX;
use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta};
use crate::health::health_checker::{Health, HealthCheckerError, Healthy};
use k8s_openapi::{
    Metadata, NamespaceResourceScope, Resource, apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::api::TypeMeta;
use kube::core::{Expression, Selector, SelectorExt};
use std::{any::Any, sync::Arc};

pub mod daemon_set;
pub mod deployment;
pub mod helm_release;
pub mod instrumentation;
pub mod stateful_set;

/// Represents supported resources types for health check.
pub(super) enum ResourceType {
    HelmRelease,
    InstrumentationCRD,
}

/// Error returned when trying build a [ResourceType] from an unsupported [TypeMeta].
pub(super) struct UnsupportedResourceType;

impl TryFrom<&TypeMeta> for ResourceType {
    type Error = UnsupportedResourceType;

    fn try_from(value: &TypeMeta) -> Result<Self, Self::Error> {
        if value == &helmrelease_v2_type_meta() {
            Ok(ResourceType::HelmRelease)
        } else if value == &instrumentation_v1beta1_type_meta() {
            Ok(ResourceType::InstrumentationCRD)
        } else {
            Err(UnsupportedResourceType)
        }
    }
}

/// Executes the provided health-check function over the items provided. It expects a list
/// of `Arc<K>` because k8s reflectors provide shared references.
/// It returns:
/// * A healthy result if the result of execution is healthy for all the items.
/// * The first encountered error or unhealthy result, otherwise.
pub(super) fn check_health_for_items<K, F>(
    items: impl Iterator<Item = Arc<K>>,
    health_check_fn: F,
) -> Result<Health, HealthCheckerError>
where
    K: Any + Clone,
    F: Fn(&K) -> Result<Health, HealthCheckerError>,
{
    for arc_obj in items {
        let obj: &K = &arc_obj; // Dereference so the function see the object and not the Arc.
        let obj_health = health_check_fn(obj)?;
        if !obj_health.is_healthy() {
            return Ok(obj_health);
        }
    }
    Ok(Healthy::new(String::default()).into())
}

/// Returns a closure which can be used as filter predicate. It will filter objects labeled with the key
/// [LABEL_RELEASE_FLUX] and the provided release name as value.
pub(super) fn flux_release_filter<K>(release_name: String) -> impl Fn(&Arc<K>) -> bool
where
    K: Metadata<Ty = ObjectMeta>,
{
    let selector = Selector::from(Expression::Equal(
        LABEL_RELEASE_FLUX.to_string(),
        release_name,
    ));

    move |obj| {
        obj.metadata()
            .labels
            .as_ref()
            .is_some_and(|labels| selector.matches(labels))
    }
}

/// Helper to return an error when an expected field in the StatefulSet object is missing.
pub(super) fn missing_field_error<K>(_: &K, name: &str, field: &str) -> HealthCheckerError
where
    K: Resource<Scope = NamespaceResourceScope>,
{
    HealthCheckerError::MissingK8sObjectField {
        kind: K::KIND.to_string(),
        name: name.to_string(),
        field: field.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::health_checker::Unhealthy;
    use assert_matches::assert_matches;
    use k8s_openapi::api::core::v1::Pod;

    #[test]
    fn test_items_health_check_healthy() {
        let items = vec!["a", "b", "c", "d"].into_iter().map(Arc::new);
        let result = check_health_for_items(items.into_iter(), |_| Ok(Healthy::default().into()))
            .unwrap_or_else(|err| panic!("unexpected error {err} when all items are healthy"));
        assert_eq!(
            result,
            Health::Healthy(Healthy::default()),
            "Expected healthy when all items are healthy"
        );

        let result = check_health_for_items(Vec::<Arc<i32>>::new().into_iter(), |_: &i32| {
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
        let items = vec!["a", "b", "c", "d"].into_iter().map(Arc::new);
        let result = check_health_for_items(items.into_iter(), |s| match s {
            &"a" | &"b" => Ok(Healthy::default().into()),
            _ => Ok(Unhealthy::new(String::default(), s.to_string()).into()),
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
        let items = vec!["a", "b", "c", "d"].into_iter().map(Arc::new);
        let result = check_health_for_items(items.into_iter(), |s| match s {
            &"a" | &"b" => Ok(Healthy::default().into()),
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
}
