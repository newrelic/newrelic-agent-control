use super::definition::{Reflector, ReflectorBuilder};
use crate::k8s::error::K8sError;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{
    Metadata, NamespaceResourceScope, Resource,
    api::apps::v1::{DaemonSet, Deployment, StatefulSet},
};
use serde::de::DeserializeOwned;
use std::{fmt::Debug, sync::Arc};

/// We assume that any error on the watcher for a standard k8s resource is recoverable, so the
/// reflector should never be stopped for that reason. Also if we stopped there is no current
/// mechanism to re-initialize it again.
const STD_WATCHER_STOP_POLICY: bool = false;

/// The `ResourceWithReflector` trait represents Kubernetes resources that have a namespace scope.
/// It includes metadata and traits required for Kubernetes object reflection and caching.
///
/// # Type Parameters
///   - Implement the `Resource` trait with a `NamespaceResourceScope`.
///   - Have a static `ObjectMeta` metadata type through `Metadata`.
///   - Be capable of being deserialized via `DeserializeOwned`.
///   - Be clonable, debuggable, and thread-safe (`Send` and `Sync`).
///
/// By implementing this trait for various Kubernetes resources like DaemonSet, Deployment,
/// and StatefulSet, we ensure that they can be managed using a generic pattern. Besides, we ensure
/// that reflectors can only be built for supported types.
pub trait ResourceWithReflector:
    Resource<Scope = NamespaceResourceScope>
    + Clone
    + DeserializeOwned
    + Debug
    + Metadata<Ty = ObjectMeta>
    + Send
    + Sync
    + 'static
{
}

// K8s resources with reflectors
impl ResourceWithReflector for DaemonSet {}
impl ResourceWithReflector for Deployment {}
impl ResourceWithReflector for StatefulSet {}

/// Gathers together the reflectors for resources implementing [ResourceWithReflector]
pub struct Reflectors {
    pub deployment: Reflector<Deployment>,
    pub daemon_set: Reflector<DaemonSet>,
    pub stateful_set: Reflector<StatefulSet>,
}

impl Reflectors {
    pub async fn try_new(builder: &ReflectorBuilder) -> Result<Reflectors, K8sError> {
        Ok(Reflectors {
            deployment: builder.try_build(STD_WATCHER_STOP_POLICY).await?,
            daemon_set: builder.try_build(STD_WATCHER_STOP_POLICY).await?,
            stateful_set: builder.try_build(STD_WATCHER_STOP_POLICY).await?,
        })
    }
}

impl<K: ResourceWithReflector> Reflector<K> {
    pub fn list(&self) -> Vec<Arc<K>> {
        self.reader().state()
    }
}
