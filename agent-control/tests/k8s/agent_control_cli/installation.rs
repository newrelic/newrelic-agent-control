use std::{collections::BTreeMap, time::Duration};

use assert_cmd::Command;
use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::{Api, api::PostParams};

use crate::{common::retry::retry, k8s::tools::k8s_env::K8sEnv};

// This test can break if the chart introduces any breaking changes.
// If this situation occurs, we will need to disable the test or use
// a similar workaround than the one we use in the tiltfile.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some("test-secret".to_string()),
            namespace: Some(namespace.clone()),
            ..Default::default()
        },
        string_data: Some(BTreeMap::from([(
            "values.yaml".to_string(),
            serde_json::json!({
                "config": {
                    "fleet_control": {
                        "enabled": false,
                    },
                    "subAgents": {},
                },
                "global": {
                    "cluster": "test-cluster",
                    "licenseKey": "***",
                },
            })
            .to_string(),
        )])),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(k8s_env.client.clone(), &namespace);
    runtime
        .block_on(secrets.create(&PostParams::default(), &secret))
        .unwrap();

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    // This chart version must be a valid version of the "agent-control-deployment" chart
    cmd.arg("--chart-version").arg("*");
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().success();

    let pods: Api<Pod> = Api::namespaced(k8s_env.client.clone(), &namespace);
    retry(10, Duration::from_secs(1), || {
        let _ = runtime.block_on(pods.get("test-release-agent-control"));
        Ok(())
    });
}
