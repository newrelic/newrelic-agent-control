use crate::common::{
    check_deployments_exist, create_local_sa_config, create_mock_config_maps, k8s_env, retry,
    start_super_agent,
};
use k8s_test_env::runtime::block_on;
use newrelic_super_agent::k8s::store::STORE_KEY_LOCAL_DATA_CONFIG;
use std::time::Duration;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_sub_agent_started() {
    let test_name = "k8s_sub_agent_started";
    // Setup k8s env
    let mut k8s = block_on(k8s_env());
    let namespace = block_on(k8s.test_namespace());

    let file_path = create_local_sa_config(namespace.as_str(), "no-opamp", test_name);
    block_on(create_mock_config_maps(
        k8s.client.clone(),
        namespace.as_str(),
        test_name,
        "local-data-my-agent-id",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));
    block_on(create_mock_config_maps(
        k8s.client.clone(),
        namespace.as_str(),
        test_name,
        "local-data-my-agent-id-2",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));

    let mut child = start_super_agent(file_path.as_ref());

    let deployment_name = "my-agent-id-opentelemetry-collector";
    let deployment_name_2 = "my-agent-id-2-opentelemetry-collector";

    // Check deployment for first Agent is created with retry.

    retry(30, Duration::from_secs(5), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &[deployment_name, deployment_name_2],
            namespace.as_str(),
        ))
    });

    child.kill().expect("Failed to kill child process");

    // TODO Clean resources after finish when working with this test in the future.
}
