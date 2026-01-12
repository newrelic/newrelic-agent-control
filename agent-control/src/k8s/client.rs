use super::{dynamic_object::DynamicObjectManagers, error::K8sError, reflectors::ReflectorBuilder};
use crate::agent_control::config::{
    daemonset_type_meta, deployment_type_meta, statefulset_type_meta,
};
use crate::k8s::dynamic_object::TypeMetaNamespaced;
use crate::k8s::utils::{get_namespace, get_type_meta};
use either::Either;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{APIResourceList, ObjectMeta};
use kube::api::ObjectList;
use kube::api::entry::Entry;
use kube::core::Status;
use kube::{
    Api, Client, Config, Resource,
    api::{DeleteParams, ListParams, PostParams},
    config::KubeConfigOptions,
    core::{DynamicObject, TypeMeta},
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::{collections::BTreeMap, sync::Arc};
use tokio::runtime::Runtime;
use tracing::debug;

/// Provides a _sync_ implementation of [AsyncK8sClient].
///
/// It offers a sync version of each async method implemented in the [AsyncK8sClient]. To do so,
/// it essentially calls to `runtime.block_on(self.async_client.future)` using the holt runtime reference.
///
/// Its maintainability can be improved using a procedural macro to generate all the methods implementation
/// automatically.
///
/// This implementation allows us to encapsulate the use of a runtime to perform async calls from synchronous code.
/// Besides, the names are explicit (Sync/Async prefixes) and the async client implementation is also public because
/// we are still analyzing whole the asynchronous runtime should be used agent-control. Since the async client implements
/// the actual k8s requests through [kube], most integration tests (which depend on a k8s cluster) will remain unchanged
/// using the async client.
pub struct SyncK8sClient {
    async_client: AsyncK8sClient,
    runtime: Arc<Runtime>,
}

impl Debug for SyncK8sClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncK8sClient")
            .field("async_client", &"AsyncK8sClient implementation")
            .field("runtime", &self.runtime)
            .finish()
    }
}

#[cfg_attr(test, mockall::automock)]
impl SyncK8sClient {
    pub fn try_new(runtime: Arc<Runtime>) -> Result<Self, K8sError> {
        Ok(Self {
            async_client: runtime.block_on(AsyncK8sClient::try_new())?,
            runtime,
        })
    }

    pub fn list_api_resources(&self) -> Result<Vec<APIResourceList>, K8sError> {
        self.runtime
            .block_on(self.async_client.list_api_resources())
    }

    pub fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.apply_dynamic_object(obj))
    }

    pub fn apply_dynamic_object_if_changed(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.apply_dynamic_object_if_changed(obj))
    }

    pub fn patch_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
        patch: serde_json::Value,
    ) -> Result<DynamicObject, K8sError> {
        self.runtime.block_on(
            self.async_client
                .patch_dynamic_object(tm, name, namespace, patch),
        )
    }

    pub fn get_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        self.runtime
            .block_on(self.async_client.get_dynamic_object(tm, name, namespace))
    }

    pub fn delete_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<Either<DynamicObject, Status>, K8sError> {
        self.runtime
            .block_on(self.async_client.delete_dynamic_object(tm, name, namespace))
    }

    pub fn delete_dynamic_object_collection(
        &self,
        tm: &TypeMeta,
        namespace: &str,
        label_selector: &str,
    ) -> Result<Either<ObjectList<DynamicObject>, Status>, K8sError> {
        self.runtime
            .block_on(self.async_client.delete_dynamic_object_collection(
                tm,
                namespace,
                label_selector,
            ))
    }

    pub fn list_dynamic_objects(
        &self,
        tm: &TypeMeta,
        ns: &str,
    ) -> Result<Vec<Arc<DynamicObject>>, K8sError> {
        self.runtime
            .block_on(self.async_client.list_dynamic_objects(tm, ns))
    }

    pub fn has_dynamic_object_changed(&self, obj: &DynamicObject) -> Result<bool, K8sError> {
        self.runtime
            .block_on(self.async_client.has_dynamic_object_changed(obj))
    }

    pub fn delete_configmap_collection(
        &self,
        namespace: &str,
        label_selector: &str,
    ) -> Result<(), K8sError> {
        self.runtime.block_on(
            self.async_client
                .delete_configmap_collection(namespace, label_selector),
        )
    }

    pub fn get_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        self.runtime
            .block_on(self.async_client.get_configmap_key(name, namespace, key))
    }

    // Gets the decoded secret key assuming it contains a String.
    pub fn get_secret_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        self.runtime
            .block_on(self.async_client.get_secret_key(name, namespace, key))
    }

    pub fn set_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        labels: BTreeMap<String, String>,
        key: &str,
        value: &str,
    ) -> Result<(), K8sError> {
        self.runtime.block_on(
            self.async_client
                .set_configmap_key(name, namespace, labels, key, value),
        )
    }

    pub fn delete_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.delete_configmap_key(name, namespace, key))
    }

    pub fn list_stateful_set(&self, ns: &str) -> Result<Vec<Arc<StatefulSet>>, K8sError> {
        self.runtime
            .block_on(self.async_client.list_stateful_set(ns))
    }

    pub fn list_daemon_set(&self, ns: &str) -> Result<Vec<Arc<DaemonSet>>, K8sError> {
        self.runtime.block_on(self.async_client.list_daemon_set(ns))
    }

    pub fn list_deployment(&self, ns: &str) -> Result<Vec<Arc<Deployment>>, K8sError> {
        self.runtime.block_on(self.async_client.list_deployment(ns))
    }
}

pub struct AsyncK8sClient {
    client: Client,
    dynamic_object_managers: DynamicObjectManagers,
}

impl AsyncK8sClient {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    pub async fn try_new() -> Result<Self, K8sError> {
        debug!("trying inClusterConfig for k8s client");

        let config = match Config::incluster() {
            Ok(c) => c,
            Err(e) => {
                debug!("inClusterConfig {}, trying kubeconfig for k8s client", e);
                let c = KubeConfigOptions::default();
                Config::from_kubeconfig(&c).await?
            }
        };

        // This test is a workaround in place to avoid long hangs https://github.com/kube-rs/kube/issues/1796
        // Once that it is properly fixed, this can be removed and we could leverage the same client for both.
        let reflector_client = Client::try_from(config.clone())?;
        let reflector_builder = ReflectorBuilder::new(reflector_client);

        let client = Client::try_from(config)?;
        debug!("k8s client initialization succeeded");
        Ok(Self {
            client: client.clone(),
            dynamic_object_managers: DynamicObjectManagers::new(client, reflector_builder),
        })
    }

    // Due to the Kube-rs library we need to retrieve with two different calls the versions of each object and then fetch the available kinds
    pub async fn list_api_resources(&self) -> Result<Vec<APIResourceList>, K8sError> {
        let mut list = vec![];
        for v in self.client.list_core_api_versions().await?.versions {
            let new = self.client.list_core_api_resources(v.as_str()).await?;
            list.push(new);
        }

        for v in self.client.list_api_groups().await?.groups {
            let new = self
                .client
                .list_api_group_resources(
                    v.preferred_version
                        .or_else(|| v.versions.first().cloned())
                        .unwrap_or_default()
                        .group_version
                        .as_str(),
                )
                .await?;
            list.push(new);
        }

        Ok(list)
    }

    /// Gets the decoded secret key assuming it contains a String.
    pub async fn get_secret_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        let secret_client = Api::<Secret>::namespaced(self.client.clone(), namespace);

        let Some(secret) = secret_client.get_opt(name).await? else {
            debug!("Secret {}:{} not found", namespace, name);
            return Ok(None);
        };

        let Some(data) = secret.data else {
            debug!("Secret {}:{} missing data", namespace, name);
            return Ok(None);
        };

        let Some(value) = data.get(key) else {
            debug!("Secret {}:{} missing key {}", namespace, name, key);
            return Ok(None);
        };

        let v = std::str::from_utf8(&value.0)
            .map_err(|e| K8sError::Generic(format!("decoding secret key: {e}")))?;
        Ok(Some(v.to_string()))
    }

    pub async fn delete_configmap_collection(
        &self,
        namespace: &str,
        label_selector: &str,
    ) -> Result<(), K8sError> {
        let api: Api<ConfigMap> = Api::<ConfigMap>::namespaced(self.client.clone(), namespace);

        delete_collection(&api, label_selector).await?;
        Ok(())
    }

    pub async fn get_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        let cm_client: Api<ConfigMap> =
            Api::<ConfigMap>::namespaced(self.client.clone(), namespace);

        let Some(cm) = cm_client.get_opt(name).await? else {
            debug!("ConfigMap {}:{} not found", namespace, name);
            return Ok(None);
        };

        let Some(data) = cm.data else {
            debug!("ConfigMap {}:{} missing data", namespace, name);
            return Ok(None);
        };

        let Some(value) = data.get(key) else {
            debug!("ConfigMap {}:{} missing key {}", namespace, name, key);
            return Ok(None);
        };

        Ok(Some(value.clone()))
    }

    pub async fn set_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        labels: BTreeMap<String, String>,
        key: &str,
        value: &str,
    ) -> Result<(), K8sError> {
        let cm_client: Api<ConfigMap> =
            Api::<ConfigMap>::namespaced(self.client.clone(), namespace);
        cm_client
            .entry(name)
            .await?
            .or_insert(|| ConfigMap {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    labels: Some(labels.clone()),
                    ..ObjectMeta::default()
                },
                ..Default::default()
            })
            .and_modify(|cm| {
                cm.metadata.labels = Some(labels);
                cm.data
                    .get_or_insert_with(BTreeMap::default)
                    .insert(key.to_string(), value.to_string());
            })
            .commit(&PostParams::default())
            .await?;
        Ok(())
    }

    pub async fn delete_configmap_key(
        &self,
        name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), K8sError> {
        let cm_client: Api<ConfigMap> =
            Api::<ConfigMap>::namespaced(self.client.clone(), namespace);
        let entry = cm_client.entry(name).await?.and_modify(|cm| {
            if let Some(mut data) = cm.data.clone() {
                data.remove(key);
                cm.data = Some(data)
            }
        });

        match entry {
            Entry::Occupied(mut e) => {
                e.commit(&PostParams::default()).await?;
            }
            Entry::Vacant(_) => {}
        }
        Ok(())
    }

    pub async fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        let tmn = &TypeMetaNamespaced::new(&get_type_meta(obj)?, &get_namespace(obj)?);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .apply(obj)
            .await
    }

    pub async fn list_stateful_set(&self, ns: &str) -> Result<Vec<Arc<StatefulSet>>, K8sError> {
        self.list_resource(&statefulset_type_meta(), ns).await
    }

    pub async fn list_daemon_set(&self, ns: &str) -> Result<Vec<Arc<DaemonSet>>, K8sError> {
        self.list_resource(&daemonset_type_meta(), ns).await
    }

    pub async fn list_deployment(&self, ns: &str) -> Result<Vec<Arc<Deployment>>, K8sError> {
        self.list_resource(&deployment_type_meta(), ns).await
    }

    async fn list_resource<K: Resource + for<'a> serde::Deserialize<'a>>(
        &self,
        tm: &TypeMeta,
        ns: &str,
    ) -> Result<Vec<Arc<K>>, K8sError> {
        self.dynamic_object_managers
            .get_or_create(&TypeMetaNamespaced::new(tm, ns))
            .await?
            .list()
            .iter()
            .map(|d| {
                Arc::unwrap_or_clone(d.clone())
                    .try_parse::<K>()
                    .map_err(|err| K8sError::ParseDynamic(err.to_string(), tm.kind.to_string()))
                    .map(|obj| Arc::new(obj))
            })
            .collect()
    }

    pub async fn apply_dynamic_object_if_changed(
        &self,
        obj: &DynamicObject,
    ) -> Result<(), K8sError> {
        let tmn = &TypeMetaNamespaced::new(&get_type_meta(obj)?, &get_namespace(obj)?);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .apply_if_changed(obj)
            .await
    }

    pub async fn patch_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
        patch: serde_json::Value,
    ) -> Result<DynamicObject, K8sError> {
        let tmn = &TypeMetaNamespaced::new(tm, namespace);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .patch(name, namespace, patch)
            .await
    }

    pub async fn get_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        let tmn = &TypeMetaNamespaced::new(tm, namespace);

        Ok(self
            .dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .get(name))
    }

    pub async fn delete_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<Either<DynamicObject, Status>, K8sError> {
        let tmn = &TypeMetaNamespaced::new(tm, namespace);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .delete(name, namespace)
            .await
    }

    pub async fn delete_dynamic_object_collection(
        &self,
        tm: &TypeMeta,
        namespace: &str,
        label_selector: &str,
    ) -> Result<Either<ObjectList<DynamicObject>, Status>, K8sError> {
        let tmn = &TypeMetaNamespaced::new(tm, namespace);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .delete_collection(namespace, label_selector)
            .await
    }

    pub async fn list_dynamic_objects(
        &self,
        tm: &TypeMeta,
        ns: &str,
    ) -> Result<Vec<Arc<DynamicObject>>, K8sError> {
        let tmn = &TypeMetaNamespaced::new(tm, ns);

        Ok(self
            .dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .list())
    }

    pub async fn has_dynamic_object_changed(&self, obj: &DynamicObject) -> Result<bool, K8sError> {
        let tmn = &TypeMetaNamespaced::new(&get_type_meta(obj)?, &get_namespace(obj)?);

        self.dynamic_object_managers
            .get_or_create(tmn)
            .await?
            .has_changed(obj)
    }
}

//  delete_collection has been moved outside the client to be able to use mockall in the client
//  without having to make K 'static.
pub(super) async fn delete_collection<K>(
    api: &Api<K>,
    label_selector: &str,
) -> Result<Either<ObjectList<K>, Status>, K8sError>
where
    K: Resource + Clone + DeserializeOwned + Debug,
{
    let result = api
        .delete_collection(
            &DeleteParams::default(),
            &ListParams {
                label_selector: Some(label_selector.to_string()),
                ..Default::default()
            },
        )
        .await?;

    match result.as_ref() {
        // List of objects being deleted.
        either::Left(list) => {
            list.iter().for_each(|obj| {
                debug!(
                    "Deleting collection: {:?}/{:?}",
                    list.types.kind,
                    obj.meta().name
                );
            });
        }
        // Status response of the deleted objects.
        either::Right(status) => {
            debug!("Deleted collection: status={:?}", status);
        }
    }

    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::k8s::utils::{get_name, get_target_namespace};
    use k8s_openapi::serde_json;
    use kube::Client;
    use tower_test::mock;

    // This test checks that an unexpected api-server error response when building reflectors doesn't
    // make the k8s client creation fail.
    // The reflector can recover itself from api-server recoverable errors, but when hasn't been
    // initialized, the request is not retried until it times-out.
    // The timeout is set to 290s since greater values can cause issues
    // (check <https://github.com/kube-rs/kube/issues/146> for details), and
    // lower values would make the reflectors poll the cluster too often. That is the reason why the [ReflectorBuilder]
    // includes a lower timeout for initialization and a retry mechanism.
    #[tokio::test]
    async fn test_client_build_success_with_temporal_deployment_api_error() {
        // It would panic if k8s client build failed.
        let _client = get_mocked_client(Scenario::FirstDeploymentRequestError).await;
    }

    async fn get_mocked_client(scenario: Scenario) -> AsyncK8sClient {
        let (mock_service, handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<kube::client::Body>>();
        ApiServerVerifier(handle).run(scenario);
        let client = Client::new(mock_service, "default");

        let reflector_builder = ReflectorBuilder::new(client.clone());
        AsyncK8sClient {
            client: client.clone(),
            dynamic_object_managers: DynamicObjectManagers::new(client.clone(), reflector_builder),
        }
    }

    type ApiServerHandle =
        mock::Handle<http::Request<kube::client::Body>, http::Response<kube::client::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    pub(crate) enum Scenario {
        FirstDeploymentRequestError,
    }

    impl ApiServerVerifier {
        fn run(mut self, scenario: Scenario) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                match scenario {
                    Scenario::FirstDeploymentRequestError => {
                        let mut first_deployments_request = true;
                        loop {
                            let (read, send) =
                                self.0.next_request().await.expect("service not called");

                            if first_deployments_request
                                && read.uri().to_string().contains("/deployments")
                            {
                                first_deployments_request = false;
                                send.send_response(
                                    http::Response::builder()
                                        .status(500)
                                        .body(kube::client::Body::empty())
                                        .unwrap(),
                                );
                                continue;
                            }
                            Self::send_expected_response(read, send)
                        }
                    }
                }
            })
        }

        fn send_expected_response(
            read: http::Request<kube::client::Body>,
            send: mock::SendResponse<http::Response<kube::client::Body>>,
        ) {
            let data = match read.uri().to_string().as_str() {
                "/apis/newrelic.com/v1" => ApiServerVerifier::get_api_resource(),
                s if s.contains("/foos?&limit=500") => ApiServerVerifier::get_watch_foo_data(),
                s if s.contains("watch=true") => serde_json::json!({}), // Empty response means no updates
                s if s.contains("test_name_create") => ApiServerVerifier::get_create_resource(),
                s if s.contains("/deployments") => ApiServerVerifier::get_deployment_data(),
                s if s.contains("/daemonsets") => ApiServerVerifier::get_daemonset_data(),
                s if s.contains("/statefulsets") => ApiServerVerifier::get_statefulset_data(),
                _ => ApiServerVerifier::get_not_found(),
            };

            let response = serde_json::to_vec(&data).unwrap();

            send.send_response(
                http::Response::builder()
                    .body(kube::client::Body::from(response))
                    .unwrap(),
            );
        }

        fn get_watch_foo_data() -> serde_json::Value {
            serde_json::json!({
              "apiVersion": "newrelic.com/v1",
              "items": [],
              "kind": "FooList",
              "metadata": {
                "continue": "",
                "resourceVersion": "207976"
              }
            }
            )
        }

        fn get_not_found() -> serde_json::Value {
            serde_json::json!(
                "Error from server (NotFound): the server could not find the requested resource"
            )
        }

        fn get_create_resource() -> serde_json::Value {
            serde_json::json!(
                            {
              "apiVersion": "newrelic.com/v1",
              "kind": "Foo",
              "metadata": {
                "creationTimestamp": "2023-12-11T21:39:38Z",
                "generation": 1,
                "managedFields": [
                  {
                    "apiVersion": "newrelic.com/v1",
                    "fieldsType": "FieldsV1",
                    "fieldsV1": {
                      "f:spec": {
                        ".": {},
                        "f:data": {}
                      }
                    },
                  }
                ],
                "name": "test_name_create",
                "namespace": "default",
                "resourceVersion": "286247",
                "uid": "97605c1d-d9a4-4202-897c-b8c8b3a0d227"
              },
              "spec": {
                "data": "test"
              }
            }
                        )
        }

        /// generated after CRD creation with kubectl get --raw /apis/newrelic.com/v1
        fn get_api_resource() -> serde_json::Value {
            serde_json::json!({
              "kind": "APIResourceList",
              "apiVersion": "v1",
              "groupVersion": "newrelic.com/v1",
              "resources": [
                {
                  "name": "foos",
                  "singularName": "foo",
                  "namespaced": true,
                  "kind": "Foo",
                  "verbs": ["delete","get","create"], // simplified
                  "storageVersionHash": "PhxIpEAAgRo="
                }
              ]
            })
        }

        fn get_deployment_data() -> serde_json::Value {
            serde_json::json!(
                {
                    "kind": "DeploymentList",
                    "apiVersion": "apps/v1",
                    "metadata": {
                      "resourceVersion": "123456",
                      "continue": "",
                    },
                    "items": [
                        {
                            "kind": "Deployment",
                            "apiVersion": "apps/v1",
                            "metadata": {
                                "name": "test-deployment",
                                "namespace": "default",
                                "resourceVersion": "123456",
                                "uid": "unique-deployment-uid"
                            }
                        },
                    ]
                }
            )
        }

        fn get_daemonset_data() -> serde_json::Value {
            serde_json::json!(
                {
                    "kind": "DaemonList",
                    "apiVersion": "apps/v1",
                    "metadata": {
                      "resourceVersion": "123456",
                      "continue": ""
                    },
                    "items": [
                        {
                            "kind": "DaemonSet",
                            "apiVersion": "apps/v1",
                            "metadata": {
                                "name": "test-daemonset",
                                "namespace": "default",
                                "resourceVersion": "123456",
                                "uid": "unique-daemonset-uid"
                            }
                        }
                    ]
                }
            )
        }

        fn get_statefulset_data() -> serde_json::Value {
            serde_json::json!(
                {
                    "kind": "StatefulSetList",
                    "apiVersion": "apps/v1",
                    "metadata": {
                        "resourceVersion": "123456",
                        "continue": ""
                    },
                    "items": []
                }
            )
        }
    }

    #[test]
    fn test_helpers() {
        let obj = &DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some("test-name".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "targetNamespace": "test",
                }
            }),
        };

        assert_eq!(get_namespace(obj).unwrap(), "default");
        assert_eq!(get_type_meta(obj).unwrap(), helmrelease_v2_type_meta());
        assert_eq!(get_name(obj).unwrap(), "test-name");
        assert_eq!(get_target_namespace(obj).unwrap(), "test");
    }
}
