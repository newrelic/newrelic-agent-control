use super::{
    error::K8sError,
    error::K8sError::UnexpectedKind,
    reader::{DynamicObjectReflector, ReflectorBuilder},
};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
use kube::{
    api::{DeleteParams, ListParams, PostParams},
    config::KubeConfigOptions,
    core::{DynamicObject, GroupVersion, ObjectMeta, TypeMeta},
    Api, Client, Config, Resource, ResourceExt,
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
};
use tracing::{debug, warn};

pub struct K8sExecutor {
    client: Client,
    dynamics: HashMap<TypeMeta, Dynamic>,
}

struct Dynamic {
    object_api: Api<DynamicObject>,
    object_reflector: DynamicObjectReflector,
}

#[cfg_attr(test, mockall::automock)]
impl K8sExecutor {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    ///
    ///
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

        debug!("verifying default namespace existence");
        Api::<Namespace>::all(client.clone())
            .get(client.default_namespace())
            .await
            .map_err(|e| {
                K8sError::UnableToSetupClient(format!("failed to get the default namespace: {}", e))
            })?;

        debug!("client creation succeeded");
        Ok(Self {
            client,
            dynamics: HashMap::new(),
        })
    }

    pub async fn try_new_with_reflectors(
        namespace: String,
        cr_type_metas: Vec<TypeMeta>,
    ) -> Result<Self, K8sError> {
        Self::try_new(namespace)
            .await?
            .with_dynamics_objects(cr_type_metas)
            .await
    }

    async fn with_dynamics_objects(
        mut self,
        cr_type_metas: Vec<TypeMeta>,
    ) -> Result<Self, K8sError> {
        let reflector_builder = ReflectorBuilder::new(self.client.to_owned());

        for tm in cr_type_metas.iter() {
            let gvk = &GroupVersion::from_str(tm.api_version.as_str())?.with_kind(tm.kind.as_str());

            let (ar, _) = match kube::discovery::pinned_kind(&self.client, gvk).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        "The gvk '{:?}' was not found in the cluster and cannot be used: {}",
                        gvk, e
                    );
                    continue;
                }
            };

            self.dynamics.insert(
                tm.to_owned(),
                Dynamic {
                    object_api: Api::default_namespaced_with(self.client.to_owned(), &ar),
                    object_reflector: reflector_builder.dynamic_object_reflector(&ar).await?,
                },
            );
        }
        Ok(self)
    }

    pub fn supported_type_meta_collection(&self) -> Vec<TypeMeta> {
        self.dynamics.keys().cloned().collect()
    }

    pub async fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        let tm = get_type_meta(obj)?;
        let name = get_name(obj)?;
        let api = &self
            .dynamics
            .get(&tm)
            .ok_or(UnexpectedKind(format!("applying dynamic object {:?}", tm)))?
            .object_api;

        // We are getting and modifying the object, but if not available we are creating it
        api.entry(name.as_str())
            .await
            .map_err(|e| {
                K8sError::GetDynamic(format!("getting dynamic object with name {}: {}", name, e))
            })?
            .and_modify(|obj_old| {
                obj_old.data = obj.data.clone();

                // TODO not updating metadata for now as we cannot overwrite everything
                // obj_old.metadata. = obj.clone().metadata;
            })
            .or_insert(|| obj.clone())
            .commit(&PostParams::default())
            .await?;
        Ok(())
    }

    pub async fn has_dynamic_object_changed(&self, obj: &DynamicObject) -> Result<bool, K8sError> {
        let name = get_name(obj)?;
        let tm = get_type_meta(obj)?;
        let existing_obj = self.get_dynamic_object(tm, name.as_str()).await?;

        match existing_obj {
            None => Ok(true),
            Some(obj_old) => {
                if obj_old.data != obj.data {
                    return Ok(true);
                }
                Ok(false)
            }
        }
    }

    pub async fn delete_dynamic_object(&self, tm: TypeMeta, name: &str) -> Result<(), K8sError> {
        let api = &self
            .dynamics
            .get(&tm)
            .ok_or(UnexpectedKind(format!("deleting dynamic object {:?}", tm)))?
            .object_api;

        match api.delete(name, &DeleteParams::default()).await? {
            // List of objects being deleted.
            either::Left(dynamic_object) => {
                debug!("Deleting object: {:?}", dynamic_object.meta().name);
            }
            // Status response of the deleted objects.
            either::Right(status) => {
                debug!("Deleted collection: status={:?}", status);
            }
        }
        Ok(())
    }

    pub async fn get_dynamic_object(
        &self,
        tm: TypeMeta,
        name: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        let reflector = &self
            .dynamics
            .get(&tm)
            .ok_or(UnexpectedKind(format!("getting dynamic object {:?}", tm)))?
            .object_reflector;

        Ok(reflector
            .reader()
            .find(|obj| obj.metadata.name.to_owned().is_some_and(|n| n.eq(name))))
    }

    pub async fn delete_dynamic_object_collection(
        &self,
        tm: TypeMeta,
        label_selector: &str,
    ) -> Result<(), K8sError> {
        let api = &self
            .dynamics
            .get(&tm)
            .ok_or(UnexpectedKind(format!(
                "deleting dynamic object collection {:?}",
                tm
            )))?
            .object_api;

        delete_collection(api, label_selector).await
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
        let cm_res = cm_client.get_opt(configmap_name).await?;

        match cm_res {
            Some(cm) => {
                let data = cm.data.ok_or(K8sError::CMMalformed())?;
                let value = data.get(key).ok_or(K8sError::KeyIsMissing())?;

                Ok(Some(value.to_string()))
            }
            None => Ok(None),
        }
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
                    // TODO The deployment should be the owner of this object
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
            .commit(&kube::api::PostParams::default())
            .await?;
        Ok(())
    }

    pub fn default_namespace(&self) -> &str {
        self.client.default_namespace()
    }
}

//  delete_collection has been moved outside the executor to be able to use mockall in the executor
//  without having to make K 'static.
async fn delete_collection<K>(api: &Api<K>, label_selector: &str) -> Result<(), K8sError>
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
            debug!(
                "Deleting collection: {:?}",
                list.iter().map(ResourceExt::name_any).collect::<Vec<_>>()
            );
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

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use assert_matches::assert_matches;
    use k8s_openapi::serde_json;
    use kube::Client;
    use tower_test::mock;

    #[tokio::test]
    async fn test_create_dynamic_object_fail_when_missing_resource_definition() {
        let tm = TypeMeta {
            api_version: "newrelic.com/v1".to_string(),
            kind: "Foo".to_string(),
        };
        let k = get_mocked_client(Scenario::APIResource)
            .with_dynamics_objects(vec![tm.clone()])
            .await
            .unwrap();

        let tm = TypeMeta {
            api_version: "missing_group/ver".to_string(),
            kind: "kind".to_string(),
        };
        let err = k
            .apply_dynamic_object(&DynamicObject {
                types: Some(tm),
                metadata: ObjectMeta {
                    name: Some("test_name".to_string()),
                    ..Default::default()
                },
                data: Default::default(),
            })
            .await
            .err()
            .unwrap();

        assert_matches!(err, UnexpectedKind(_));
    }

    #[tokio::test]
    async fn test_create_dynamic_object_succeeds_and_ignores_missing_kind() {
        let tm = TypeMeta {
            api_version: "newrelic.com/v1".to_string(),
            kind: "Foo".to_string(),
        };
        let tm_not_existing = TypeMeta {
            api_version: "not.existing/v0".to_string(),
            kind: "NotExisting".to_string(),
        };

        let k = get_mocked_client(Scenario::APIResource)
            .with_dynamics_objects(vec![tm_not_existing, tm.clone()])
            .await
            .unwrap();

        let result = k
            .apply_dynamic_object(&DynamicObject {
                types: Some(tm),
                metadata: ObjectMeta {
                    name: Some("test_name_create".to_string()),
                    ..Default::default()
                },
                data: Default::default(),
            })
            .await;

        assert!(result.is_ok());
    }

    fn get_mocked_client(scenario: Scenario) -> K8sExecutor {
        let (mock_service, handle) =
            mock::pair::<http::Request<hyper::Body>, http::Response<hyper::Body>>();
        ApiServerVerifier(handle).run(scenario);
        let client = Client::new(mock_service, "default");
        K8sExecutor {
            client,
            dynamics: HashMap::new(),
        }
    }

    type ApiServerHandle = mock::Handle<http::Request<hyper::Body>, http::Response<hyper::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    enum Scenario {
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
                        } else {
                            ApiServerVerifier::get_not_found()
                        };

                        let response = serde_json::to_vec(&data).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(hyper::Body::from(response))
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
    }
}
