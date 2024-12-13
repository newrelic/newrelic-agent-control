use crate::common::{retry::retry, runtime::block_on};
use crate::k8s::tools::agent_control::CUSTOM_AGENT_TYPE_SECRET_PATH;
use crate::k8s::tools::{
    agent_control::start_agent_control_with_testdata_config, k8s_api::check_deployments_exist,
    k8s_env::K8sEnv,
};
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;

/// This scenario checks that the agent-control is executed and creates the k8s resources,
/// including the secret that is honored to modify the Deployment Name, when OpAMP is not enabled.
#[test]
#[ignore = "needs a k8s cluster"]
#[serial]
fn k8s_sub_agent_started_with_no_opamp() {
    let test_name = "k8s_sub_agent_started";
    // Setup k8s env
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let _child = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SECRET_PATH,
        k8s.client.clone(),
        &namespace,
        None,
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    // Check deployment for first Agent is created with retry, the name has the key
    // 'override-by-secret' concatenated to the name because the secret created adds that
    // NameOverride to the values.
    retry(30, Duration::from_secs(1), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-override-by-secret"],
            namespace.as_str(),
        ))
    });
}
