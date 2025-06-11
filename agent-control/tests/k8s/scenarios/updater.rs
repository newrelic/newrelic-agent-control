use crate::common::runtime::{block_on, tokio_runtime};
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
use std::sync::Arc;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_run_updater() {
    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());

    let current_version = "1.2.3-beta".to_string();
    let new_version = "1.2.3".to_string();

    let updater = K8sACUpdater::new(k8s_client.clone(), current_version.clone());

    let config_to_update = &AgentControlDynamicConfig {
        agents: Default::default(),
        chart_version: Some(new_version.clone()),
    };
    assert!(
        updater.should_update(config_to_update),
        "Updater should indicate that an update is needed"
    );

    k8s_client
        .apply_dynamic_object(&DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(RELEASE_NAME.to_string()),
                namespace: Some(test_ns.clone()),
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

    let obj = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
        .expect("no error is expected during fetching the helm release")
        .unwrap();

    assert_eq!(
        obj.data
            .get("spec")
            .unwrap()
            .clone()
            .get("chart")
            .unwrap()
            .get("spec")
            .unwrap()
            .get("version")
            .unwrap()
            .as_str()
            .unwrap(),
        new_version.as_str()
    )
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_run_updater_missing_config() {
    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());

    let current_version = "1.2.3-beta".to_string();

    let updater = K8sACUpdater::new(k8s_client.clone(), current_version.clone());

    let config_to_udate = &AgentControlDynamicConfig {
        agents: Default::default(),
        chart_version: None,
    };

    assert!(
        !updater.should_update(config_to_udate),
        "Updater should indicate that an update is not needed since chart_version is not specified"
    );
    updater
        .update(config_to_udate)
        .expect_err("an error should occur during update when chart_version is not specified");
}
