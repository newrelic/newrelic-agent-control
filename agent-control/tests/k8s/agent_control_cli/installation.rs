use assert_cmd::Command;

use crate::k8s::tools::k8s_env::K8sEnv;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let values = "{'config':{'fleet_control':{'enabled':false},'subAgents':{}},'global':{'cluster':'test-cluster','licenseKey':'***'}}";
    let _create_secret = Command::new("minikube")
        .args([
            "kubectl",
            "--",
            "create",
            "secret",
            "generic",
            "test-secret",
        ])
        .args([format!("--from-literal=values.yaml={}", values)])
        .args(["-n", &namespace])
        .unwrap();

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg("test-release");
    // This chart version must be a valid version of the "agent-control-deployment" chart
    cmd.arg("--chart-version").arg("*");
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().success();

    for _ in 0..10 {
        let get_pods = Command::new("minikube")
            .args(["kubectl", "--", "get", "pods", "-n", &namespace])
            .unwrap();
        if String::from_utf8_lossy(&get_pods.stdout).contains("test-release-agent-control") {
            return;
        }

        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    panic!("test-release-agent-control pod not found");
}
