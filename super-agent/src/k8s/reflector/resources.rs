use super::definition::{Reflector, ReflectorBuilder};
use crate::k8s::error::K8sError;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{
    api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet},
    Metadata, NamespaceResourceScope, Resource,
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;

/// The `ResourceWithReflector` trait represents Kubernetes resources that have a namespace scope.
/// It includes metadata and traits required for Kubernetes object reflection and caching.
///
/// # Type Parameters
///   - Implement the `Resource` trait with a `NamespaceResourceScope`.
///   - Have a static `ObjectMeta` metadata type through `Metadata`.
///   - Be capable of being deserialized via `DeserializeOwned`.
///   - Be clonable, debuggable, and thread-safe (`Send` and `Sync`).
///
/// By implementing this trait for various Kubernetes resources like DaemonSet, Deployment, ReplicaSet,
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
impl ResourceWithReflector for ReplicaSet {}
impl ResourceWithReflector for StatefulSet {}

/// Gathers together the reflectors for resources implementing [ResourceWithReflector]
pub struct Reflectors {
    pub deployment: Reflector<Deployment>,
    pub daemon_set: Reflector<DaemonSet>,
    pub replica_set: Reflector<ReplicaSet>,
    pub stateful_set: Reflector<StatefulSet>,
}

impl Reflectors {
    pub async fn try_new(builder: &ReflectorBuilder) -> Result<Reflectors, K8sError> {
        Ok(Reflectors {
            deployment: builder.try_build().await?,
            daemon_set: builder.try_build().await?,
            replica_set: builder.try_build().await?,
            stateful_set: builder.try_build().await?,
        })
    }
}
