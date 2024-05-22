use super::{
    dynamic_object::{DynamicObjectManager, DynamicObjectManagers},
    error::K8sError,
    reflector::{definition::ReflectorBuilder, resources::Reflectors},
};
use crate::super_agent::config::helm_release_type_meta;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::entry::Entry;
use kube::api::ObjectList;
use kube::{
    api::{DeleteParams, ListParams, PostParams},
    config::KubeConfigOptions,
    core::{DynamicObject, TypeMeta},
    Api, Client, Config, Resource,
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
/// we are still analyzing whole the asynchronous runtime should be used super-agent. Since the async client implements
/// the actual k8s requests through [kube], most integration tests (which depend on a k8s cluster) will remain unchanged
/// using the async client.
pub struct SyncK8sClient {
    async_client: AsyncK8sClient,
    runtime: Arc<Runtime>,
}

#[cfg_attr(test, mockall::automock)]
impl SyncK8sClient {
    pub fn try_new(
        runtime: Arc<Runtime>,
        namespace: String,
        cr_type_metas: Vec<TypeMeta>,
    ) -> Result<Self, K8sError> {
        Ok(Self {
            async_client: runtime.block_on(AsyncK8sClient::try_new(namespace, cr_type_metas))?,
            runtime,
        })
    }

    /// helper to get the dynamic resource corresponding to a dynamic object.
    fn dynamic_object_manager<'a>(
        &'a self,
        obj: &DynamicObject,
    ) -> Result<&'a DynamicObjectManager, K8sError> {
        let type_meta = get_type_meta(obj)?;
        self.async_client
            .dynamic_object_managers
            .try_get(&type_meta)
    }

    pub fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.dynamic_object_manager(obj)?.apply(obj))
    }

    pub fn has_dynamic_object_changed(&self, obj: &DynamicObject) -> Result<bool, K8sError> {
        self.dynamic_object_manager(obj)?.has_changed(obj)
    }

    pub fn apply_dynamic_object_if_changed(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.dynamic_object_manager(obj)?.apply_if_changed(obj))
    }

    pub fn get_dynamic_object(
        &self,
        tm: &TypeMeta,
        name: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        Ok(self
            .async_client
            .dynamic_object_managers
            .try_get(tm)?
            .get(name))
    }

    pub fn delete_dynamic_object_collection(
        &self,
        tm: &TypeMeta,
        label_selector: &str,
    ) -> Result<(), K8sError> {
        self.runtime.block_on(
            self.async_client
                .dynamic_object_managers
                .try_get(tm)?
                .delete_by_label_selector(label_selector),
        )
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

    pub fn get_helm_release(&self, name: &str) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        let tm = helm_release_type_meta();
        self.get_dynamic_object(&tm, name)
    }

    pub fn delete_configmap_key(&self, configmap_name: &str, key: &str) -> Result<(), K8sError> {
        self.runtime
            .block_on(self.async_client.delete_configmap_key(configmap_name, key))
    }

    pub fn supported_type_meta_collection(&self) -> Vec<TypeMeta> {
        self.async_client
            .dynamic_object_managers()
            .supported_dynamic_type_metas()
    }

    pub fn list_stateful_set(&self) -> Result<ObjectList<StatefulSet>, K8sError> {
        self.runtime.block_on(self.async_client.list_stateful_set())
    }

    pub fn default_namespace(&self) -> &str {
        self.async_client.default_namespace()
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
    pub async fn try_new(namespace: String, type_meta: Vec<TypeMeta>) -> Result<Self, K8sError> {
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

        debug!("verifying default namespace existence");
        Api::<Namespace>::all(client.clone())
            .get(client.default_namespace())
            .await
            .map_err(|e| {
                K8sError::UnableToSetupClient(format!("failed to get the default namespace: {}", e))
            })?;

        debug!("client creation succeeded");
        let reflector_builder = ReflectorBuilder::new(client.clone());
        Ok(Self {
            client: client.clone(),
            reflectors: Reflectors::try_new(&reflector_builder).await?,
            dynamic_object_managers: DynamicObjectManagers::try_new(
                type_meta,
                &client,
                &reflector_builder,
            )
            .await?,
        })
    }

    pub fn dynamic_object_managers(&self) -> &DynamicObjectManagers {
        &self.dynamic_object_managers
    }

    pub fn reflectors(&self) -> &Reflectors {
        &self.reflectors
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
                ..ListParams::default()
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
    obj.metadata.clone().name.ok_or(K8sError::MissingName())
}

pub fn get_type_meta(obj: &DynamicObject) -> Result<TypeMeta, K8sError> {
    obj.types.clone().ok_or(K8sError::MissingKind())
}

/// This function returns true if there are labels and they contain the provided key, value.
pub fn contains_label_with_value(
    labels: &Option<BTreeMap<String, String>>,
    key: &str,
    value: &str,
) -> bool {
    labels
        .as_ref()
        .and_then(|labels| labels.get(key))
        .map_or(false, |v| v.as_str() == value)
}

#[cfg(test)]
pub(crate) mod test {
    use crate::k8s::reflector::resources::ResourceWithReflector;

    use super::*;
    use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
    use k8s_openapi::serde_json;
    use kube::runtime::reflector;
    use kube::Client;
    use tower_test::mock;

    #[tokio::test]
    async fn test_client_build_with_reflectors_and_get_resources() {
        let async_client = get_mocked_client(Scenario::APIResource, vec![]).await;

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

    async fn get_mocked_client(scenario: Scenario, dynamic_types: Vec<TypeMeta>) -> AsyncK8sClient {
        let (mock_service, handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<kube::client::Body>>();
        ApiServerVerifier(handle).run(scenario);
        let client = Client::new(mock_service, "default");

        let reflector_builder = ReflectorBuilder::new(client.clone());
        AsyncK8sClient {
            client: client.clone(),
            reflectors: Reflectors::try_new(&reflector_builder).await.unwrap(),
            dynamic_object_managers: DynamicObjectManagers::try_new(
                dynamic_types,
                &client,
                &reflector_builder,
            )
            .await
            .unwrap(),
        }
    }

    type ApiServerHandle =
        mock::Handle<http::Request<kube::client::Body>, http::Response<kube::client::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    pub(crate) enum Scenario {
        APIResource,
    }

    impl ApiServerVerifier {
        fn run(mut self, scenario: Scenario) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                match scenario {
                    Scenario::APIResource => loop {
                        let (read, send) = self.0.next_request().await.expect("service not called");

                        let data = if read.uri().to_string().eq("/apis/newrelic.com/v1") {
                            ApiServerVerifier::get_api_resource()
                        } else if read.uri().to_string().contains("/foos?&limit=500") {
                            ApiServerVerifier::get_watch_foo_data()
                        } else if read.uri().to_string().contains("watch=true") {
                            // Empty response mean no updates
                            serde_json::json!({})
                        } else if read.uri().to_string().contains("test_name_create") {
                            ApiServerVerifier::get_create_resource()
                        } else if read.uri().to_string().contains("/deployments") {
                            ApiServerVerifier::get_deployment_data()
                        } else if read.uri().to_string().contains("/daemonsets") {
                            ApiServerVerifier::get_daemonset_data()
                        } else if read.uri().to_string().contains("/replicasets") {
                            ApiServerVerifier::get_replicaset_data()
                        } else if read.uri().to_string().contains("/statefulsets") {
                            ApiServerVerifier::get_statefulset_data()
                        } else {
                            ApiServerVerifier::get_not_found()
                        };

                        let response = serde_json::to_vec(&data).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(kube::client::Body::from(response))
                                .unwrap(),
                        );
                    },
                }
            })
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

        fn get_replicaset_data() -> serde_json::Value {
            serde_json::json!(
                {
                    "kind": "ReplicaSetList",
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
    fn test_contains_label_with_value() {
        struct TestCase<'a> {
            name: &'a str,
            labels: &'a Option<BTreeMap<String, String>>,
            key: &'a str,
            value: &'a str,
            expected: bool,
        }

        impl TestCase<'_> {
            fn run(&self) {
                assert_eq!(
                    self.expected,
                    contains_label_with_value(self.labels, self.key, self.value),
                    "{}",
                    self.name
                )
            }
        }

        let test_cases = [
            TestCase {
                name: "No labels",
                labels: &None,
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Empty labels",
                labels: &Some(BTreeMap::default()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "No matching label",
                labels: &Some([("a".to_string(), "b".to_string())].into()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Matching label with different value",
                labels: &Some([("key".to_string(), "other".to_string())].into()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Matching label and value",
                labels: &Some([("key".to_string(), "value".to_string())].into()),
                key: "key",
                value: "value",
                expected: true,
            },
        ];

        test_cases.iter().for_each(|tc| tc.run());
    }
}
