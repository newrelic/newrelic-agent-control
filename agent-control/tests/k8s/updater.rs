use super::tools::k8s_env::K8sEnv;
use crate::common::runtime::{block_on, tokio_runtime};
use assert_cmd::Command;
use newrelic_agent_control::agent_control::config::{
    AgentControlDynamicConfig, helmrelease_v2_type_meta,
};
use newrelic_agent_control::agent_control::updater::{K8sUpdater, Updater};
use newrelic_agent_control::k8s::client::SyncK8sClient;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_updater_poc() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("agent-control");
    cmd.arg("--chart-version").arg("0.0.47");
    cmd.arg("--skip-installation-check");
    cmd.arg("--installation-check-timeout").arg("1m");
    cmd.arg("--namespace").arg(test_ns.as_str());
    cmd.assert().success();

    println!("AC installed");

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());

    let updater = K8sUpdater::new(k8s_client.clone());

    let dyn_config = AgentControlDynamicConfig {
        chart_version: "*".to_string(),
        ..Default::default()
    };

    updater.update(&dyn_config).unwrap();

    println!("AC updated");

    sleep(Duration::from_secs(10));

    // Assert release data
    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), "agent-control")
        .unwrap()
        .unwrap();

    assert_eq!(release.data["spec"]["chart"]["spec"]["version"], "*");
}
