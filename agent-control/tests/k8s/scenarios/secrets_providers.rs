use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_PATH, start_agent_control_with_testdata_config,
};
use crate::k8s::tools::k8s_api::check_helmrelease_spec_values;
use crate::k8s::tools::k8s_env::K8sEnv;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_template_secrets() {
    let test_name = "k8s_template_secrets";

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    k8s.port_forward("vault-0", 8200, 8200);
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        &namespace,
        None,
        None,
        // This config is intended to be empty
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    // Check the HelmRelease is created with the secrets correctly populated
    let expected_spec_values = r#"
hashicorpVaultV1Key: bar1
hashicorpVaultV2Key: bar2
    "#;

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))
    });
}
