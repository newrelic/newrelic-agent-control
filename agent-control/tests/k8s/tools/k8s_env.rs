use crate::common::runtime::tokio_runtime;

use super::test_crd::create_foo_crd;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::rbac::v1::ClusterRole;
use kube::{
    Api, Client,
    api::{DeleteParams, PostParams},
};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use std::{env, sync::Once};

pub const KUBECONFIG_PATH: &str = "tests/k8s/.kubeconfig-dev";

pub static INIT_RUSTLS: Once = Once::new();

/// This struct represents a running k8s cluster and it provides utilities to handle multiple namespaces, and
/// resources are cleaned-up when the object is dropped.
/// The `Foo` CR is created automatically, therefore any test using this component can assume it exits.
pub struct K8sEnv {
    pub client: Client,
    generated_namespaces: Vec<String>,
}

impl K8sEnv {
    pub async fn new() -> Self {
        INIT_RUSTLS.call_once(|| {
            install_rustls_default_crypto_provider();
        });

        // Forces the client to use the dev kubeconfig file.
        unsafe { env::set_var("KUBECONFIG", KUBECONFIG_PATH) };

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
        test_namespace.metadata.generate_name = Some("ac-test-".to_string());

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
            let cr_api: Api<ClusterRole> = Api::all(self.client.clone());

            let generated_namespaces = self.generated_namespaces.clone();
            tokio_runtime()
                .spawn(async move {
                    for ns in generated_namespaces.into_iter() {
                        ns_api
                            .delete(ns.as_str(), &DeleteParams::default())
                            .await
                            .expect("fail to remove namespace");
                    }

                    // TODO This is a workaround. As soon as we have a way to configure RELEASE_NAME in the tests, we can remove this.
                    let _ = cr_api
                        .delete(
                            "agent-control-deployment-resources",
                            &DeleteParams::default(),
                        )
                        .await;
                })
                .await
                .unwrap();
        })
    }
}
