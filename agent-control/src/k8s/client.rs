use super::{
    dynamic_object::DynamicObjectManagers,
    error::K8sError,
    reflector::{definition::ReflectorBuilder, resources::Reflectors},
};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::ObjectList;
use kube::api::entry::Entry;
use kube::{
    Api, Client, Config, Resource,
    api::{DeleteParams, ListParams, PostParams},
    config::KubeConfigOptions,
    core::{DynamicObject, TypeMeta},
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
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

#[cfg_attr(test, mockall::automock)]
impl SyncK8sClient {
    pub fn try_new(runtime: Arc<Runtime>, namespace: String) -> Result<Self, K8sError> {
        Ok(Self {
            async_client: runtime.block_on(AsyncK8sClient::try_new(namespace))?,
            runtime,
        })
    }

    pub fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.dynamic_object_managers.apply(obj))
    }

    pub fn has_dynamic_object_changed(&self, obj: &DynamicObject) -> Result<bool, K8sError> {
        self.runtime
            .block_on(self.async_client.dynamic_object_managers.has_changed(obj))
    }

    pub fn apply_dynamic_object_if_changed(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime.block_on(
            self.async_client
                .dynamic_object_managers
                .apply_if_changed(obj),
        )
    }

    pub fn get_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        self.runtime
            .block_on(self.async_client.dynamic_object_managers.get(tm, name))
    }
    pub fn delete_dynamic_object(&self, tm: &TypeMeta, name: &str) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.dynamic_object_managers.delete(tm, name))
    }

    pub fn list_dynamic_objects(&self, tm: &TypeMeta) -> Result<Vec<Arc<DynamicObject>>, K8sError> {
        self.runtime
            .block_on(self.async_client.dynamic_object_managers.list(tm))
    }

    pub fn delete_configmap_collection(&self, label_selector: &str) -> Result<(), K8sError> {
        self.runtime.block_on(
            self.async_client
                .delete_configmap_collection(label_selector),
        )
    }

    pub fn get_configmap_key(
        &self,
        configmap_name: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        self.runtime
            .block_on(self.async_client.get_configmap_key(configmap_name, key))
    }

    pub fn set_configmap_key(
        &self,
        configmap_name: &str,
        labels: BTreeMap<String, String>,
        key: &str,
        value: &str,
    ) -> Result<(), K8sError> {
        self.runtime.block_on(self.async_client.set_configmap_key(
            configmap_name,
            labels,
            key,
            value,
        ))
    }

    pub fn delete_configmap_key(&self, configmap_name: &str, key: &str) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.delete_configmap_key(configmap_name, key))
    }

    pub fn default_namespace(&self) -> &str {
        self.async_client.default_namespace()
    }

    /// Returns the stateful_set list using the corresponding reflector.
    pub fn list_stateful_set(&self) -> Vec<Arc<StatefulSet>> {
        self.async_client.reflectors.stateful_set.list()
    }

    /// Returns the daemon_set list using the corresponding reflector.
    pub fn list_daemon_set(&self) -> Vec<Arc<DaemonSet>> {
        self.async_client.reflectors.daemon_set.list()
    }

    /// Returns the deployment list using the corresponding reflector.
    pub fn list_deployment(&self) -> Vec<Arc<Deployment>> {
        self.async_client.reflectors.deployment.list()
    }
}

pub struct AsyncK8sClient {
    client: Client,
    reflectors: Reflectors,
    dynamic_object_managers: DynamicObjectManagers,
}

impl AsyncK8sClient {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    pub async fn try_new(namespace: String) -> Result<Self, K8sError> {
        debug!("trying inClusterConfig for k8s client");

        let mut config = match Config::incluster() {
            Ok(c) => c,
            Err(e) => {
                debug!(
                    "inClusterConfig failed {}, trying kubeconfig for k8s client",
                    e
                );
                let c = KubeConfigOptions::default();
                Config::from_kubeconfig(&c).await?
            }
        };

        config.default_namespace = namespace;
        let client = Client::try_from(config)?;

        debug!("verifying default k8s namespace existence");
        Api::<Namespace>::all(client.clone())
            .get(client.default_namespace())
            .await
            .map_err(|e| {
                K8sError::UnableToSetupClient(format!("failed to get the default namespace: {}", e))
            })?;

        let reflector_builder = ReflectorBuilder::new(client.clone());
        let reflectors = Reflectors::try_new(&reflector_builder).await?;

        debug!("k8s client initialization succeeded");
        Ok(Self {
            client: client.clone(),
            reflectors,
            dynamic_object_managers: DynamicObjectManagers::new(client.clone(), reflector_builder),
        })
    }

    pub fn dynamic_object_managers(&self) -> &DynamicObjectManagers {
        &self.dynamic_object_managers
    }

    pub async fn delete_configmap_collection(&self, label_selector: &str) -> Result<(), K8sError> {
        let api: Api<ConfigMap> = Api::<ConfigMap>::default_namespaced(self.client.clone());

        delete_collection(&api, label_selector).await
    }

    pub async fn get_configmap_key(
        &self,
        configmap_name: &str,
        key: &str,
    ) -> Result<Option<String>, K8sError> {
        let cm_client: Api<ConfigMap> = Api::<ConfigMap>::default_namespaced(self.client.clone());

        if let Some(cm) = cm_client.get_opt(configmap_name).await? {
            if let Some(data) = cm.data {
                if let Some(key) = data.get(key) {
                    return Ok(Some(key.clone()));
                }
                debug!("ConfigMap {} missing key {}", configmap_name, key)
            } else {
                debug!("ConfigMap {} missing data", configmap_name)
            }
        } else {
            debug!("ConfigMap {} not found", configmap_name)
        }

        Ok(None)
    }

    pub async fn set_configmap_key(
        &self,
        configmap_name: &str,
        labels: BTreeMap<String, String>,
        key: &str,
        value: &str,
    ) -> Result<(), K8sError> {
        let cm_client: Api<ConfigMap> = Api::<ConfigMap>::default_namespaced(self.client.clone());
        cm_client
            .entry(configmap_name)
            .await?
            .or_insert(|| ConfigMap {
                metadata: ObjectMeta {
                    name: Some(configmap_name.to_string()),
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
        configmap_name: &str,
        key: &str,
    ) -> Result<(), K8sError> {
        let cm_client: Api<ConfigMap> = Api::<ConfigMap>::default_namespaced(self.client.clone());
        let entry = cm_client.entry(configmap_name).await?.and_modify(|cm| {
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

    pub async fn list_stateful_set(&self) -> Result<ObjectList<StatefulSet>, K8sError> {
        let ss_client: Api<StatefulSet> =
            Api::<StatefulSet>::default_namespaced(self.client.clone());
        let list_stateful_set = ss_client.list(&ListParams::default()).await?;

        Ok(list_stateful_set)
    }

    pub fn default_namespace(&self) -> &str {
        self.client.default_namespace()
    }
}

//  delete_collection has been moved outside the client to be able to use mockall in the client
//  without having to make K 'static.
pub(super) async fn delete_collection<K>(api: &Api<K>, label_selector: &str) -> Result<(), K8sError>
where
    K: Resource + Clone + DeserializeOwned + Debug,
{
    match api
        .delete_collection(
            &DeleteParams::default(),
            &ListParams {
                label_selector: Some(label_selector.to_string()),
                ..Default::default()
            },
        )
        .await?
    {
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

    Ok(())
}

pub fn get_name(obj: &DynamicObject) -> Result<String, K8sError> {
    obj.metadata.clone().name.ok_or(K8sError::MissingCRName)
}

pub fn get_type_meta(obj: &DynamicObject) -> Result<TypeMeta, K8sError> {
    obj.types.clone().ok_or(K8sError::MissingCRKind)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::k8s::reflector::resources::ResourceWithReflector;
    use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
    use k8s_openapi::serde_json;
    use kube::Client;
    use kube::runtime::reflector;
    use tower_test::mock;

    #[tokio::test]
    async fn test_client_build_with_reflectors_and_get_resources() {
        let async_client = get_mocked_client(Scenario::APIResource).await;

        let deployment_reader = async_client.reflectors.deployment.reader();
        let daemonset_reader = async_client.reflectors.daemon_set.reader();

        fn find_resource_by_name<K>(reader: &reflector::Store<K>, name: &str) -> Option<Arc<K>>
        where
            K: ResourceWithReflector,
        {
            reader.find(|resource| resource.metadata().name.as_deref() == Some(name))
        }

        // Check for an existing deployment
        let existing_deployment =
            find_resource_by_name::<Deployment>(&deployment_reader, "test-deployment");
        assert!(
            existing_deployment.is_some(),
            "Expected deployment to be found"
        );

        // Check for an existing daemonset
        let existing_daemonset =
            find_resource_by_name::<DaemonSet>(&daemonset_reader, "test-daemonset");
        assert!(
            existing_daemonset.is_some(),
            "Expected daemonset to be found"
        );

        // Check for a non-existent deployment
        let non_existent_deployment =
            find_resource_by_name::<Deployment>(&deployment_reader, "unexistent-deployment");
        assert!(
            non_existent_deployment.is_none(),
            "Expected no deployment to be found"
        );
    }

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
            reflectors: Reflectors::try_new(&reflector_builder).await.unwrap(),
            dynamic_object_managers: DynamicObjectManagers::new(client.clone(), reflector_builder),
        }
    }

    type ApiServerHandle =
        mock::Handle<http::Request<kube::client::Body>, http::Response<kube::client::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    pub(crate) enum Scenario {
        APIResource,
        FirstDeploymentRequestError,
    }

    impl ApiServerVerifier {
        fn run(mut self, scenario: Scenario) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                match scenario {
                    Scenario::APIResource => loop {
                        let (read, send) = self.0.next_request().await.expect("service not called");
                        Self::send_expected_response(read, send);
                    },
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
}
