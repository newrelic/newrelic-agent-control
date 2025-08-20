use super::tools::k8s_api::create_values_secret;
use super::tools::k8s_env::K8sEnv;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::instance_id;
use crate::k8s::tools::local_chart::{LOCAL_CHART_REPOSITORY, agent_control_deploymet::*};
use crate::k8s::tools::logs::{AC_LABEL_SELECTOR, print_pod_logs};
use crate::k8s::tools::opamp::get_minikube_opamp_url_from_fake_server;
use assert_cmd::Command;
use kube::Client;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::process::Stdio;
use std::thread::sleep;
use std::time::Duration;
use url::Url;

const POLL_INTERVAL: u64 = 5;

const SECRET_NAME: &str = "ac-values";
const VALUES_KEY: &str = "values.yaml";

const NEW_FLUX_VERSION: &str = "flux2-1.0.0";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_health_check_for_flux_when_fails() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_DEV_1,
    );

    let agents_config = r#"agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.1.0
"#;

    let ac_config = format!(
        r#"
{agents_config}
cd_chart_version: {NEW_FLUX_VERSION}
"#
    );
    opamp_server.set_config_response(ac_instance_id.clone(), ac_config.as_str());

    retry(60, Duration::from_secs(5), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });

    uninstall_helm_release("agent-control-cd", "default");

    retry(60, Duration::from_secs(5), || {
        let result = check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id);
        match result {
            Err(_) => {
                println!("Success: The expected unhealthy state was detected.");
                Ok(())
            }
            Ok(_) => Err("Health status is still healthy, retrying...".into()),
        }
    });
}

/// installs ac with cli using minimal values set, pointing it to fake opamp server and printing
/// pod logs to stdout
fn bootstrap_ac(
    client: Client,
    opamp_server: &FakeServer,
    namespace: &str,
    chart_version: &str,
) -> InstanceID {
    let opamp_endpoint = get_minikube_opamp_url_from_fake_server(opamp_server.endpoint().as_str());

    print_pod_logs(client.clone(), namespace, AC_LABEL_SELECTOR);

    create_values_secret(
        client.clone(),
        namespace,
        SECRET_NAME,
        VALUES_KEY,
        ac_chart_values(opamp_endpoint, namespace),
    );

    install_ac_with_cli(namespace, chart_version);

    // make some OpAMP seq number gap between old and new pod to avoid the fake server to
    // always send full-resend flag for each pod, and finally keep the new pod data once
    // the old pod die.
    sleep(Duration::from_secs(POLL_INTERVAL * 4));

    instance_id::get_instance_id(client.clone(), namespace, &AgentID::AgentControl)
}

fn install_ac_with_cli(namespace: &str, chart_version: &str) {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();

    cmd.arg("install-agent-control");
    cmd.arg("--log-level").arg("debug");
    cmd.arg("--repository-url").arg(LOCAL_CHART_REPOSITORY);
    cmd.arg("--chart-name").arg("agent-control-deployment");
    cmd.arg("--chart-version").arg(chart_version);
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--secrets")
        .arg(format!("{SECRET_NAME}={VALUES_KEY}"));
    cmd.arg("--skip-installation-check");
    cmd.assert().success();
}

// Default image and tag will be used, in the case of local the default is overwritten in Tilt
// setting it to tilt.local/ac-dev:dev
fn ac_chart_values(opamp_endpoint: Url, name_override: &str) -> String {
    serde_json::json!({
        // give a unique name per test to the cluster role to avoid collisions
        "nameOverride": name_override,
        "acRemoteUpdate": false,
        "cdRemoteUpdate": false,
        "config": {
          // Disable the SI creation
          "fleet_control": {
              "enabled": false,
          },
          "agentControl": {
            "content": {
              "log": {
                "level":"debug",
              },
              // To make health assertions faster
              "health_check":{
                "initial_delay": "1s",
                "interval": "20s",
              },
              "fleet_control": {
                "endpoint": opamp_endpoint.as_str(),
                "poll_interval": format!("{POLL_INTERVAL}s"),
                "signature_validation": {
                  "enabled": "false",
                },
              },
            },
          }
        },
        "global": {
          "cluster": "test-cluster",
          "licenseKey": "***",
        },
    })
    .to_string()
}

fn uninstall_helm_release(release_name: &str, namespace: &str) {
    println!("helm uninstallation {release_name} -n {namespace}");

    let status = std::process::Command::new("helm")
        .arg("uninstall")
        .arg(release_name)
        .arg("--namespace")
        .arg(namespace)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("Failed to execute helm command");

    if !status.success() {
        panic!("Command 'helm uninstall {release_name}' failed with exit code {status}");
    }

    println!("Uninstallation of '{release_name}' complete.");
}
