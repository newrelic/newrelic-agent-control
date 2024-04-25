use k8s_openapi::api::core::v1::Namespace;
use kube::{
    api::{DeleteParams, PostParams},
    Api, Client,
};
use std::env;

use crate::{foo_crd::create_foo_crd, runtime::tokio_runtime};

/// This struct represents a running k8s cluster and it provides utilities to handle multiple namespaces, and
/// resources are cleaned-up when the object is dropped.
pub struct K8sEnv {
    /// [kube::Client] which can be safely cloned
    pub client: Client,
    generated_namespaces: Vec<String>,
}

impl K8sEnv {
    pub async fn new(kubeconfig_path: &str) -> Self {
        // Forces the client to use the dev kubeconfig file.
        env::set_var("KUBECONFIG", kubeconfig_path);

        let client = Client::try_default().await.expect("fail to create client");
        create_foo_crd(client.to_owned()).await;

        K8sEnv {
            client,
            generated_namespaces: Vec::new(),
        }
    }

    /// Creates and returns a namespace for testing purposes, it will be deleted when the [K8sEnv] object is dropped.
    pub async fn test_namespace(&mut self) -> String {
        let mut test_namespace = Namespace::default();
        test_namespace.metadata.generate_name = Some("super-agent-test-".to_string());

        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        let created_namespace = namespaces
            .create(&PostParams::default(), &test_namespace)
            .await
            .expect("fail to create test namespace");

        let ns = created_namespace
            .metadata
            .name
            .ok_or("fail getting the ns")
            .unwrap();

        self.generated_namespaces.push(ns.clone());

        ns
    }
}

impl Drop for K8sEnv {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // 'async drop' doesn't exist so `block_on` is needed to run it synchronously.
        //
        // Since K8sEnv variables can be dropped from either sync or async code, we need an additional runtime to make
        // it work.
        //
        // `futures::executor::block_on` is needed because we cannot execute `runtime.block_on` from a tokio
        // context (such as `#[tokio::test]`) as it would fail with:
        // ```
        // 'Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.'
        // ````
        // It is important to notice that the usage of `futures::executor::block_on` could lead to a dead-lock if there
        // are not available threads in the tokio runtime, so we need to use the multi-threading version of the macro:
        // `#[tokio::test(flavor = "multi_thread")]`
        //
        // `runtime.spawn(<future-block>).await` is needed because we cannot execute `futures::executor::block_on` when there is
        // no tokio runtime (synchronous tests), since it would fail with:
        // ```
        // 'there is no reactor running, must be called from the context of a Tokio 1.x runtime
        // ```
        futures::executor::block_on(async move {
            let ns_api: Api<Namespace> = Api::all(self.client.clone());
            let generated_namespaces = self.generated_namespaces.clone();
            tokio_runtime()
                .spawn(async move {
                    for ns in generated_namespaces.into_iter() {
                        ns_api
                            .delete(ns.as_str(), &DeleteParams::default())
                            .await
                            .expect("fail to remove namespace");
                    }
                })
                .await
                .unwrap();
        })
    }
}
