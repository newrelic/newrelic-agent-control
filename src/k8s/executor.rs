use k8s_openapi::api::core::v1::Pod;
use kube::config::KubeConfigOptions;
use kube::core::DynamicObject;
use kube::{api::ListParams, core::GroupVersionKind, Api, Client, Config};
use mockall::*;
// use std::collections::HashMap;
use super::error::K8sError;
use tracing::debug;

#[derive(Clone)]
pub struct K8sExecutor {
    client: Client,
    // reflectors_cache: HashMap<GroupVersionKind, K8sReflector>,
}

#[automock]
impl K8sExecutor {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    ///
    pub async fn try_default() -> Result<K8sExecutor, K8sError> {
        debug!("trying inClusterConfig for k8s client");
        let config = Config::incluster().unwrap_or({
            debug!("inClusterConfig failed, trying kubeconfig for k8s client");
            let c = KubeConfigOptions::default();
            Config::from_kubeconfig(&c).await?
        });

        let c = Client::try_from(config)?;
        debug!("client creation succeeded");
        Ok(K8sExecutor::new(c))
    }

    pub fn new(c: Client) -> K8sExecutor {
        K8sExecutor { client: c }
    }
    // We forsee that persistant module in k8s will need some helpers here.
    // pub async fn persits_config(String)

    pub async fn create_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        spec: &str,
    ) -> Result<(), K8sError> {
        unimplemented!();
    }
    pub async fn modify_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        spec: &str,
    ) -> Result<(), K8sError> {
        unimplemented!();
    }
    pub async fn delete_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<(), K8sError> {
        unimplemented!();
    }

    // Depends on K8sReflector implementation
    pub async fn get_dynamic_object(
        &self,
        gvk: GroupVersionKind,
        name: &str,
    ) -> Result<DynamicObject, K8sError> {
        // get the right reflector from self.reflectors and get the object.
        // let r = self.get_reflector(gvk)
        unimplemented!();
    }

    // async fn get_reflector(&self, gvk: GroupVersionKind) -> K8sReflector {
    //     // get from reflectors_cache or create new one and add it to the cache.
    // }

    pub async fn get_minor_version(&self) -> Result<String, K8sError> {
        let version = self.client.apiserver_version().await?;
        Ok(version.minor)
    }

    pub async fn get_pods(&self) -> Result<Vec<Pod>, K8sError> {
        let pod_client: Api<Pod> = Api::default_namespaced(self.client.clone());
        let pod_list = pod_client.list(&ListParams::default()).await?;
        Ok(pod_list.items)
    }
}

#[cfg(test)]
mod test {
    use crate::k8s::executor::K8sExecutor;
    use k8s_openapi::serde_json;
    use kube::Client;
    use tower_test::mock;

    ///
    /// The following tests are just an example to show how the client can be mocked
    /// at a HTTP level
    ///

    #[tokio::test]
    async fn test_version_with_http_mock() {
        let k = get_mocked_client(Scenario::Version);
        let version = k.get_minor_version().await;

        assert_eq!(true, version.is_ok());
        let version = version.unwrap();
        assert_eq!(version, "24");
    }

    #[tokio::test]
    async fn test_get_pods_with_http_mock() {
        let k = get_mocked_client(Scenario::ListPods);
        let list_pods = k.get_pods().await;

        assert_eq!(true, list_pods.is_ok());
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
        let custom_client = Client::new(mock_service, "default");
        K8sExecutor::new(custom_client)
    }

    type ApiServerHandle = mock::Handle<http::Request<hyper::Body>, http::Response<hyper::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    enum Scenario {
        Version,
        ListPods,
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
