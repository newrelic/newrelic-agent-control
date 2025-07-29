use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::agent_control_cli::upgrade_local_vs_remote::check_version_and_source;
use crate::k8s::tools::k8s_env::K8sEnv;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::DynamicObject;
use newrelic_agent_control::agent_control::config::{
    AgentControlDynamicConfig, helmrelease_v2_type_meta,
};
use newrelic_agent_control::agent_control::version_updater::k8s::K8sACUpdater;
use newrelic_agent_control::agent_control::version_updater::updater::VersionUpdater;
use newrelic_agent_control::cli::install_agent_control::RELEASE_NAME;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_run_updater() {
    // set up the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());

    let current_version = "1.2.3-beta".to_string();
    let new_version = "1.2.3".to_string();

    let updater = K8sACUpdater::new(
        true,
        true,
        k8s_client.clone(),
        test_ns.clone(),
        current_version.clone(),
    );

    let config_to_update = &AgentControlDynamicConfig {
        agents: Default::default(),
        chart_version: Some(new_version.clone()),
    };

    k8s_client
        .apply_dynamic_object(&DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(RELEASE_NAME.to_string()),
                namespace: Some(test_ns.clone()),
                labels: Some(BTreeMap::from([
                    (
                        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
                        LOCAL_VAL.to_string(),
                    ),
                    ("test_key".to_string(), "test_val".to_string()),
                ])),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "interval": "5m",
                    "timeout": "5m",
                    "chart": {
                        "spec": {
                            "chart": "test",
                            "version": current_version,
                            "sourceRef": {
                                "kind": "HelmRepository",
                                "name": RELEASE_NAME,
                            },
                            "interval": "5m",
                        },
                    }
            }}),
        })
        .expect("no error should occur during the creation of the helm release");

    updater
        .update(config_to_update)
        .expect("no error should occur during update");

    retry(15, Duration::from_secs(5), || {
        let Some(obj) =
            k8s_client.get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME, &test_ns)?
        else {
            return Err("Helm Release not found".into());
        };

        if 2 != obj.metadata.clone().labels.unwrap_or_default().len() {
            return Err(format!("labels were overwritten: {obj:?}").into());
        }

        check_version_and_source(&k8s_client, new_version.as_str(), REMOTE_VAL, &test_ns)
    })
}
