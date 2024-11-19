use crate::common::{retry::retry, runtime::block_on};
use crate::k8s::tools::super_agent::CUSTOM_AGENT_TYPE_PATH;
use crate::k8s::tools::{
    k8s_api::check_deployments_exist, k8s_env::K8sEnv,
    super_agent::start_super_agent_with_testdata_config,
};
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;

/// This scenario checks that the super-agent is executed and creates the k8s resources corresponding to the
/// local configuration when OpAMP is not enabled.
#[test]
#[ignore = "needs a k8s cluster"]
#[serial]
fn k8s_sub_agent_started_with_no_opamp() {
    let test_name = "k8s_sub_agent_started";
    // Setup k8s env
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let _child = start_super_agent_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        None,
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    // Check deployment for first Agent is created with retry.
    retry(30, Duration::from_secs(5), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &["hello-world"],
            namespace.as_str(),
        ))
    });
}
