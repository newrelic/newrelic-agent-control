use super::tools::{
    k8s_api::check_deployments_exist, k8s_env::K8sEnv, retry, runtime::block_on,
    super_agent::start_super_agent_with_testdata_config,
};
use std::time::Duration;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_sub_agent_started_with_no_opamp() {
    let test_name = "k8s_sub_agent_started";
    // Setup k8s env
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let mut child = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        None,
        vec!["local-data-my-agent-id", "local-data-my-agent-id-2"],
    );

    let deployment_name = "my-agent-id-opentelemetry-collector";
    let deployment_name_2 = "my-agent-id-2-opentelemetry-collector";

    // Check deployment for first Agent is created with retry.
    let res = retry(30, Duration::from_secs(5), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &[deployment_name, deployment_name_2],
            namespace.as_str(),
        ))
    });

    child.kill().expect("Failed to kill child process");
    res.unwrap_or_else(|err| panic!("retry failed {err}"));

    // TODO Clean resources after finish when working with this test in the future.
}
