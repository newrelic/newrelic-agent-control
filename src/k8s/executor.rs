use super::{
    error::K8sError,
    reader::{DynamicObjectReflector, ReflectorBuilder},
};
use crate::k8s::Error::UnexpectedKind;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::api::{DeleteParams, PostParams};
use kube::core::DynamicObject;
use kube::core::GroupVersion;
use kube::core::TypeMeta;
use kube::{config::KubeConfigOptions, core::ObjectMeta};
use kube::{Api, Client, Config};
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tracing::debug;

pub struct K8sExecutor {
    client: Client,
    dynamic_reflectors: HashMap<TypeMeta, DynamicObjectReflector>,
    dynamic_apis: HashMap<TypeMeta, Api<DynamicObject>>,
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
        debug!("client creation succeeded");

        Ok(Self {
            client,
            dynamic_reflectors: HashMap::new(),
            dynamic_apis: HashMap::new(),
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
            let (ar, _) = kube::discovery::pinned_kind(&self.client, gvk)
                .await
                .map_err(|_| UnexpectedKind(gvk.clone().kind))?;

            let api = Api::default_namespaced_with(self.client.to_owned(), &ar);
            self.dynamic_apis.insert(tm.to_owned(), api);

            let reflector = reflector_builder.dynamic_object_reflector(&ar).await?;
            self.dynamic_reflectors.insert(tm.to_owned(), reflector);
        }
        Ok(self)
    }

    pub async fn apply_dynamic_object(&self, obj: &DynamicObject) -> Result<(), K8sError> {
        let name = obj.metadata.clone().name.ok_or(K8sError::MissingName())?;
        let tm = obj.types.clone().ok_or(K8sError::MissingKind())?;
        let api = self
            .dynamic_apis
            .get(&tm)
            .ok_or(UnexpectedKind("applying dynamic object".to_string()))?;

        // We are getting and modifying the object, but if not available we are creating it
        api.entry(name.as_str())
            .await
            .map_err(|e| K8sError::GetDynamic(e.to_string()))?
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

    pub async fn delete_dynamic_object(&self, tm: TypeMeta, name: &str) -> Result<(), K8sError> {
        let api = self
            .dynamic_apis
            .get(&tm)
            .ok_or(UnexpectedKind("applying dynamic object".to_string()))?;
        api.delete(name, &DeleteParams::default()).await?;

        Ok(())
    }

    pub async fn get_dynamic_object(
        &self,
        tm: TypeMeta,
        name: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        let r = self
            .dynamic_reflectors
            .get(&tm)
            .ok_or(UnexpectedKind("getting dynamic object".to_string()))?;

        Ok(r.reader()
            .find(|obj| obj.metadata.name.to_owned().is_some_and(|n| n.eq(name))))
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
                    // TODO The deployment should be the owner of this object
                    ..ObjectMeta::default()
                },
                ..Default::default()
            })
            .and_modify(|cm| {
                cm.data
                    .get_or_insert_with(BTreeMap::default)
                    .insert(key.to_string(), value.to_string());
            })
            .commit(&kube::api::PostParams::default())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use assert_matches::assert_matches;
    use k8s_openapi::serde_json;
    use kube::Client;
    use tower_test::mock;

    #[tokio::test]
    async fn create_dynamic_object_fail_when_missing_resource_definition() {
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
    async fn create_dynamic_object_succeeds() {
        let tm = TypeMeta {
            api_version: "newrelic.com/v1".to_string(),
            kind: "Foo".to_string(),
        };

        let k = get_mocked_client(Scenario::APIResource)
            .with_dynamics_objects(vec![tm.clone()])
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
            dynamic_reflectors: HashMap::new(),
            dynamic_apis: HashMap::new(),
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
