use super::{
    error::K8sError,
    reader::{DynamicObjectReflector, ReflectorBuilder},
};
use async_trait::async_trait;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::api::core::v1::Pod;
use kube::core::DynamicObject;
use kube::{
    api::{DeleteParams, PostParams},
    discovery::ApiResource,
};
use kube::{
    api::{ListParams, Patch, PatchParams},
    core::GroupVersionKind,
    Api, Client, Config,
};
use kube::{config::KubeConfigOptions, core::ObjectMeta};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tracing::debug;

const SA_ACTOR: &str = "super-agent-patch";

pub struct K8sExecutor {
    client: Client,
    reflector_builder: ReflectorBuilder,
    dynamic_reflectors: HashMap<ApiResource, DynamicObjectReflector>,
}

#[cfg_attr(test, mockall::automock)]
// TODO: This is just an example and once we've implemented the config, needs to be removed.
// #[derive(Error, Debug)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum K8sResourceType {
    OtelHelmRepository,
    OtelColHelmRelease,
}

// TODO: For now only the two used function are defined in the interface.
//  We might want to break into a specific dynamic objects.
// interface.
#[async_trait]
pub trait K8sDynamicObjectsManager {
    async fn create_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        spec: &str,
    ) -> Result<DynamicObject, K8sError>;
    async fn delete_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<(), K8sError>;
}

#[async_trait]
impl K8sDynamicObjectsManager for K8sExecutor {
    async fn create_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        spec: &str,
    ) -> Result<DynamicObject, K8sError> {
        let api = self.namespaced_api(gvk).await?;

        let object_spec: DynamicObject = serde_yaml::from_str(spec)?;

        let created_object = api.create(&PostParams::default(), &object_spec).await?;

        Ok(created_object)
    }

    async fn delete_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<(), K8sError> {
        let api = self.namespaced_api(gvk).await?;

        api.delete(name, &DeleteParams::default()).await?;

        Ok(())
    }
}

#[automock]
impl K8sExecutor {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    ///
    pub async fn try_default(namespace: String) -> Result<Self, K8sError> {
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

        Ok(Self::new(client))
    }

    pub fn new(client: Client) -> Self {
        let reflector_builder = ReflectorBuilder::new(client.to_owned());
        Self {
            client,
            reflector_builder,
            dynamic_reflectors: HashMap::new(),
        }
    }

    pub async fn create_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        spec: &str,
    ) -> Result<DynamicObject, K8sError> {
        let api = self.namespaced_api(gvk).await?;

        let object_spec: DynamicObject = serde_yaml::from_str(spec)?;

        let created_object = api.create(&PostParams::default(), &object_spec).await?;

        Ok(created_object)
    }

    pub async fn patch_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
        spec: &str,
    ) -> Result<(), K8sError> {
        let api = self.namespaced_api(gvk).await?;

        let object_spec: DynamicObject = serde_yaml::from_str(spec)?;

        api.patch(
            name,
            &PatchParams::apply(SA_ACTOR).force(),
            &Patch::Apply(&object_spec),
        )
        .await?;

        Ok(())
    }

    pub async fn delete_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<(), K8sError> {
        let api = self.namespaced_api(gvk).await?;

        api.delete(name, &DeleteParams::default()).await?;

        Ok(())
    }

    // TODO this is not thread safe, lock mechanism need to be added to the reflector cache, will be added if needed when
    // usage of these fn are defined.
    pub async fn get_dynamic_object(
        &mut self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<Option<Arc<DynamicObject>>, K8sError> {
        let ar = self.api_resource(gvk).await?;

        let reflector = self
            .dynamic_reflectors
            .entry(ar.to_owned())
            .or_insert(self.reflector_builder.dynamic_object_reflector(&ar).await?);

        Ok(reflector
            .reader()
            .find(|obj| obj.metadata.name.to_owned().is_some_and(|n| n.eq(name))))
    }

    pub async fn get_minor_version(&self) -> Result<String, K8sError> {
        let version = self.client.apiserver_version().await?;
        Ok(version.minor)
    }

    pub async fn get_pods(&self) -> Result<Vec<Pod>, K8sError> {
        let pod_client: Api<Pod> = Api::default_namespaced(self.client.clone());
        let pod_list = pod_client.list(&ListParams::default()).await?;
        Ok(pod_list.items)
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

    // TODO this can be cached, or specialized in a similar way as the reflectors, so is not called on each operation.
    async fn namespaced_api(&self, gvk: GroupVersionKind) -> Result<Api<DynamicObject>, K8sError> {
        Ok(Api::default_namespaced_with(
            self.client.to_owned(),
            &self.api_resource(gvk).await?,
        ))
    }

    async fn api_resource(&self, gvk: GroupVersionKind) -> Result<ApiResource, K8sError> {
        let (api_resource, _) = kube::discovery::pinned_kind(&self.client, &gvk)
            .await
            .map_err(|_| K8sError::MissingKind(gvk.api_version(), gvk.kind))?;
        Ok(api_resource)
    }
}

impl K8sResourceType {
    pub fn to_gvk(&self) -> GroupVersionKind {
        match self {
            K8sResourceType::OtelHelmRepository => GroupVersionKind {
                group: "source.toolkit.fluxcd.io".into(),
                version: "v1beta2".into(),
                kind: "HelmRepository".into(),
            },
            K8sResourceType::OtelColHelmRelease => GroupVersionKind {
                group: "helm.toolkit.fluxcd.io".into(),
                version: "v2beta1".into(),
                kind: "HelmRelease".into(),
            },
        }
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::k8s::Error;
    use assert_matches::assert_matches;
    use async_trait::async_trait;
    use k8s_openapi::serde_json;
    use kube::{core::GroupVersionKind, Client};
    use tower_test::mock;

    mock! {
        pub K8sExecutorMock {}

        #[async_trait]
        impl K8sDynamicObjectsManager for K8sExecutorMock {
            async fn create_dynamic_object(
                &self,
                gvk: GroupVersionKind,
                spec: &str,
            ) -> Result<DynamicObject, Error>;

            async fn delete_dynamic_object(
                &self,
                gvk: GroupVersionKind,
                name: &str,
            ) -> Result<(), Error>;
        }
    }

    #[tokio::test]
    async fn create_dynamic_object_fail_when_missing_resource_definition() {
        let k = get_mocked_client(Scenario::Version);

        let gvk = GroupVersionKind::gvk("missing_group", "ver", "kind");

        let err = k.create_dynamic_object(gvk.clone(), "").await.unwrap_err();

        assert_matches!(err, K8sError::MissingKind(_, _));
    }

    #[tokio::test]
    async fn create_dynamic_object_fail_when_yaml_bad_format() {
        // Mock must have the available gvk to not fail before.
        let k = get_mocked_client(Scenario::APIResource);

        let err = k
            .create_dynamic_object(
                GroupVersionKind::gvk("newrelic.com", "v1", "Foo"),
                "bad: yaml: format",
            )
            .await
            .unwrap_err();

        assert_matches!(err, K8sError::SerdeYaml(_));
    }

    #[tokio::test]
    async fn create_dynamic_object_fails_on_creation() {
        let k = get_mocked_client(Scenario::APIResource);

        let err = k
            .create_dynamic_object(
                GroupVersionKind::gvk("newrelic.com", "v1", "Foo"),
                "bad: spec",
            )
            .await
            .unwrap_err();

        assert_matches!(err, K8sError::Generic(_));
    }

    ///
    /// The following tests are just an example to show how the client can be mocked
    /// at a HTTP level
    ///

    #[tokio::test]
    async fn test_version_with_http_mock() {
        let k = get_mocked_client(Scenario::Version);
        let version = k.get_minor_version().await;

        assert!(version.is_ok());
        let version = version.unwrap();
        assert_eq!(version, "24");
    }

    #[tokio::test]
    async fn test_get_pods_with_http_mock() {
        let k = get_mocked_client(Scenario::ListPods);
        let list_pods = k.get_pods().await;

        assert!(list_pods.is_ok());
        let pods = list_pods.unwrap();
        assert_eq!(pods.len(), 1);
        pods.iter().for_each(|pod| {
            if pod.metadata.name.as_ref().unwrap() == "test" {
                assert_eq!(
                    pod.spec.as_ref().unwrap().containers[0]
                        .image
                        .as_ref()
                        .unwrap(),
                    "nginx"
                );
            }
        })
    }

    fn get_mocked_client(scenario: Scenario) -> K8sExecutor {
        let (mock_service, handle) =
            mock::pair::<http::Request<hyper::Body>, http::Response<hyper::Body>>();
        ApiServerVerifier(handle).run(scenario);
        let client = Client::new(mock_service, "default");
        K8sExecutor::new(client)
    }

    type ApiServerHandle = mock::Handle<http::Request<hyper::Body>, http::Response<hyper::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    enum Scenario {
        Version,
        ListPods,
        APIResource,
    }
    impl ApiServerVerifier {
        fn run(mut self, scenario: Scenario) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                match scenario {
                    Scenario::ListPods => {
                        let (_, send) = self.0.next_request().await.expect("service not called");
                        let response =
                            serde_json::to_vec(&ApiServerVerifier::get_list_pod_data()).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(hyper::Body::from(response))
                                .unwrap(),
                        );
                    }
                    Scenario::Version => {
                        let (_, send) = self.0.next_request().await.expect("service not called");

                        let response =
                            serde_json::to_vec(&ApiServerVerifier::get_version_data()).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(hyper::Body::from(response))
                                .unwrap(),
                        );
                    }
                    Scenario::APIResource => {
                        let (_, send) = self.0.next_request().await.expect("service not called");

                        let response =
                            serde_json::to_vec(&ApiServerVerifier::get_api_resource()).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(hyper::Body::from(response))
                                .unwrap(),
                        );
                    }
                }
            })
        }
        fn get_version_data() -> serde_json::Value {
            serde_json::json!({
              "major": "1",
              "minor": "24",
              "gitVersion": "v1.24.15-gke.1700",
              "gitCommit": "8cadcdb5605ddc1b77a0b1dd3fbd8182a23f58ae",
              "gitTreeState": "clean",
              "buildDate": "2023-07-17T09:27:42Z",
              "goVersion": "go1.19.10 X:boringcrypto",
              "compiler": "gc",
              "platform": "linux/amd64"
            })
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

        ///
        /// This function was generated running k get --raw "/api/v1/namespaces/default/pods"
        /// has 1 pod named test
        ///
        fn get_list_pod_data() -> serde_json::Value {
            serde_json::json!({
              "kind": "PodList",
              "apiVersion": "v1",
              "metadata": {
                "resourceVersion": "567467138"
              },
              "items": [
                {
                  "metadata": {
                    "name": "pod",
                    "namespace": "default",
                    "uid": "f3c61b09-179e-4edc-b607-284ac7d7bb11",
                    "resourceVersion": "567465347",
                    "creationTimestamp": "2023-10-26T07:41:51Z",
                    "labels": {
                      "run": "pod"
                    },
                    "managedFields": [
                      {
                        "manager": "kubectl-run",
                        "operation": "Update",
                        "apiVersion": "v1",
                        "time": "2023-10-26T07:41:51Z",
                        "fieldsType": "FieldsV1",
                        "fieldsV1": {
                          "f:metadata": {
                            "f:labels": {
                              ".": {},
                              "f:run": {}
                            }
                          },
                          "f:spec": {
                            "f:containers": {
                              "k:{\"name\":\"pod\"}": {
                                ".": {},
                                "f:args": {},
                                "f:image": {},
                                "f:imagePullPolicy": {},
                                "f:name": {},
                                "f:resources": {},
                                "f:terminationMessagePath": {},
                                "f:terminationMessagePolicy": {}
                              }
                            },
                            "f:dnsPolicy": {},
                            "f:enableServiceLinks": {},
                            "f:restartPolicy": {},
                            "f:schedulerName": {},
                            "f:securityContext": {},
                            "f:terminationGracePeriodSeconds": {}
                          }
                        }
                      },
                      {
                        "manager": "kubelet",
                        "operation": "Update",
                        "apiVersion": "v1",
                        "time": "2023-10-26T07:42:15Z",
                        "fieldsType": "FieldsV1",
                        "fieldsV1": {
                          "f:status": {
                            "f:conditions": {
                              "k:{\"type\":\"ContainersReady\"}": {
                                ".": {},
                                "f:lastProbeTime": {},
                                "f:lastTransitionTime": {},
                                "f:message": {},
                                "f:reason": {},
                                "f:status": {},
                                "f:type": {}
                              },
                              "k:{\"type\":\"Initialized\"}": {
                                ".": {},
                                "f:lastProbeTime": {},
                                "f:lastTransitionTime": {},
                                "f:status": {},
                                "f:type": {}
                              },
                              "k:{\"type\":\"Ready\"}": {
                                ".": {},
                                "f:lastProbeTime": {},
                                "f:lastTransitionTime": {},
                                "f:message": {},
                                "f:reason": {},
                                "f:status": {},
                                "f:type": {}
                              }
                            },
                            "f:containerStatuses": {},
                            "f:hostIP": {},
                            "f:phase": {},
                            "f:podIP": {},
                            "f:podIPs": {
                              ".": {},
                              "k:{\"ip\":\"10.108.3.162\"}": {
                                ".": {},
                                "f:ip": {}
                              }
                            },
                            "f:startTime": {}
                          }
                        },
                        "subresource": "status"
                      }
                    ]
                  },
                  "spec": {
                    "volumes": [
                      {
                        "name": "kube-api-access-x949n",
                        "projected": {
                          "sources": [
                            {
                              "serviceAccountToken": {
                                "expirationSeconds": 3607,
                                "path": "token"
                              }
                            },
                            {
                              "configMap": {
                                "name": "kube-root-ca.crt",
                                "items": [
                                  {
                                    "key": "ca.crt",
                                    "path": "ca.crt"
                                  }
                                ]
                              }
                            },
                            {
                              "downwardAPI": {
                                "items": [
                                  {
                                    "path": "namespace",
                                    "fieldRef": {
                                      "apiVersion": "v1",
                                      "fieldPath": "metadata.namespace"
                                    }
                                  }
                                ]
                              }
                            }
                          ],
                          "defaultMode": 420
                        }
                      }
                    ],
                    "containers": [
                      {
                        "name": "pod",
                        "image": "nginx",
                        "args": [
                          "test"
                        ],
                        "resources": {},
                        "volumeMounts": [
                          {
                            "name": "kube-api-access-x949n",
                            "readOnly": true,
                            "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount"
                          }
                        ],
                        "terminationMessagePath": "/dev/termination-log",
                        "terminationMessagePolicy": "File",
                        "imagePullPolicy": "Always"
                      }
                    ],
                    "restartPolicy": "Always",
                    "terminationGracePeriodSeconds": 30,
                    "dnsPolicy": "ClusterFirst",
                    "serviceAccountName": "default",
                    "serviceAccount": "default",
                    "nodeName": "gke-gke-1-19-test-pool-1-68409c31-95ik",
                    "securityContext": {},
                    "schedulerName": "default-scheduler",
                    "tolerations": [
                      {
                        "key": "node.kubernetes.io/not-ready",
                        "operator": "Exists",
                        "effect": "NoExecute",
                        "tolerationSeconds": 300
                      },
                      {
                        "key": "node.kubernetes.io/unreachable",
                        "operator": "Exists",
                        "effect": "NoExecute",
                        "tolerationSeconds": 300
                      }
                    ],
                    "priority": 0,
                    "enableServiceLinks": true,
                    "preemptionPolicy": "PreemptLowerPriority"
                  },
                  "status": {
                    "phase": "Running",
                    "conditions": [
                      {
                        "type": "Initialized",
                        "status": "True",
                        "lastProbeTime": null,
                        "lastTransitionTime": "2023-10-26T07:41:51Z"
                      },
                      {
                        "type": "Ready",
                        "status": "False",
                        "lastProbeTime": null,
                        "lastTransitionTime": "2023-10-26T07:42:15Z",
                        "reason": "ContainersNotReady",
                        "message": "containers with unready status: [pod]"
                      },
                      {
                        "type": "ContainersReady",
                        "status": "False",
                        "lastProbeTime": null,
                        "lastTransitionTime": "2023-10-26T07:42:15Z",
                        "reason": "ContainersNotReady",
                        "message": "containers with unready status: [pod]"
                      },
                      {
                        "type": "PodScheduled",
                        "status": "True",
                        "lastProbeTime": null,
                        "lastTransitionTime": "2023-10-26T07:41:51Z"
                      }
                    ],
                    "hostIP": "10.132.0.47",
                    "podIP": "10.108.3.162",
                    "podIPs": [
                      {
                        "ip": "10.108.3.162"
                      }
                    ],
                    "startTime": "2023-10-26T07:41:51Z",
                    "containerStatuses": [
                      {
                        "name": "pod",
                        "state": {
                          "waiting": {
                            "reason": "CrashLoopBackOff",
                            "message": "back-off 5m0s restarting failed container=pod pod=pod_default(f3c61b09-179e-4edc-b607-284ac7d7bb11)"
                          }
                        },
                        "lastState": {
                          "terminated": {
                            "exitCode": 1,
                            "reason": "Error",
                            "startedAt": "2023-10-26T08:33:36Z",
                            "finishedAt": "2023-10-26T08:33:36Z",
                            "containerID": "containerd://d69d9845042f40b89981801bac56ff9539fc234c0578a1361da0dcf6ac459ed3"
                          }
                        },
                        "ready": false,
                        "restartCount": 15,
                        "image": "docker.io/library/nginx:latest",
                        "imageID": "docker.io/library/nginx@sha256:add4792d930c25dd2abf2ef9ea79de578097a1c175a16ab25814332fe33622de",
                        "containerID": "containerd://d69d9845042f40b89981801bac56ff9539fc234c0578a1361da0dcf6ac459ed3",
                        "started": false
                      }
                    ],
                    "qosClass": "BestEffort"
                  }
                }
              ]
            })
        }
    }
}
