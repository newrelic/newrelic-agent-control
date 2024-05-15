use std::{collections::HashMap, str::FromStr};

use kube::{
    api::{ApiResource, DynamicObject, TypeMeta},
    core::GroupVersion,
    discovery::pinned_kind,
    Api,
};
use tracing::warn;

use super::{
    error::K8sError,
    reflector::builder::{Reflector, ReflectorBuilder},
};

/// An abstraction of [DynamicObject] that allow performing operations concerning objects known at Runtime either
/// using the k8s API or a [Reflector].
pub struct DynamicResource {
    api: Api<DynamicObject>,
    reflector: Reflector<DynamicObject>,
}

/// [DynamicResource] collection. Each resource is accessible through the corresponding [TypeMeta].
pub struct DynamicResources(HashMap<TypeMeta, DynamicResource>);

impl DynamicResource {
    pub async fn try_new(
        api_resource: &ApiResource,
        client: kube::Client,
        builder: &ReflectorBuilder,
    ) -> Result<Self, K8sError> {
        Ok(Self {
            api: Api::default_namespaced_with(client, api_resource),
            reflector: builder.try_build_with_api_resource(api_resource).await?,
        })
    }

    pub fn api(&self) -> &Api<DynamicObject> {
        &self.api
    }

    pub fn reflector(&self) -> &Reflector<DynamicObject> {
        &self.reflector
    }
}

impl DynamicResources {
    pub async fn try_new(
        dynamic_types: impl IntoIterator<Item = TypeMeta>,
        client: &kube::Client,
        builder: &ReflectorBuilder,
    ) -> Result<Self, K8sError> {
        let mut inner = HashMap::new();
        for type_meta in dynamic_types.into_iter() {
            let gvk = &GroupVersion::from_str(type_meta.api_version.as_str())?
                .with_kind(type_meta.kind.as_str());

            let api_resource = match pinned_kind(client, gvk).await {
                Ok((api_resource, _)) => api_resource,
                Err(err) => {
                    warn!(
                        "The gvk '{:?}' was not found in the cluster and cannot be used: {}",
                        gvk, err
                    );
                    continue;
                }
            };

            inner.insert(
                type_meta,
                DynamicResource::try_new(&api_resource, client.to_owned(), builder).await?,
            );
        }
        Ok(Self(inner))
    }

    pub fn supported_dynamic_type_metas(&self) -> Vec<TypeMeta> {
        self.0.keys().cloned().collect()
    }

    pub fn try_get(&self, type_meta: &TypeMeta) -> Result<&DynamicResource, K8sError> {
        let ds = self.0.get(type_meta).ok_or_else(|| {
            K8sError::UnexpectedKind(format!("no reflector for type {:?}", type_meta))
        })?;
        Ok(ds)
    }
}
