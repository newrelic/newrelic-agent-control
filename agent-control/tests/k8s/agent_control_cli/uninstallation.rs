use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::agent_control_cli::installation::ac_install_cmd;
use crate::k8s::tools::cmd::print_cli_output;
use crate::k8s::tools::k8s_api::create_values_secret;
use crate::k8s::tools::k8s_env::K8sEnv;
use crate::k8s::tools::local_chart::agent_control_deploymet::CHART_VERSION_LATEST_RELEASE;
use crate::k8s::tools::logs::{AC_LABEL_SELECTOR, print_pod_logs};
use assert_cmd::Command;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::Api;
use std::time::Duration;

#[test]
#[ignore = "needs k8s cluster"]
// This test can break if the chart introduces any breaking changes.
// If this situation occurs, we will need to disable the test or use
// a similar workaround than the one we use in the tiltfile.
fn k8s_cli_install_agent_control_installation_and_uninstallation() {
    let mut k8s_env = block_on(K8sEnv::new());
    let ac_namespace = block_on(k8s_env.test_namespace());
    let subagents_namespace = block_on(k8s_env.test_namespace());

    let values = serde_json::json!({
        "nameOverride": "",
        "cleanupManagedResources": false,
        "subAgentsNamespace": subagents_namespace,
        "config": {
            "fleet_control": {
                "enabled": false,
            },
            "agents": {
                "nrdot":{
                    "agent_type" : "newrelic/io.opentelemetry.collector:0.1.0",
                },
            }
        },
        "agentsConfig": {
            "nrdot":{
                "chart_version" : "*"
            },
        },
        "global": {
            "cluster": "test-cluster",
            "licenseKey": "thisisafakelicensekey",
        },
    })
    .to_string();
    create_values_secret(
        k8s_env.client.clone(),
        &ac_namespace,
        "test-secret",
        "values.yaml",
        values,
    );

    print_pod_logs(k8s_env.client.clone(), &ac_namespace, AC_LABEL_SELECTOR);

    let release_name = "install-ac-installation-and-uninstallation";
    let mut cmd = ac_install_cmd(
        &ac_namespace,
        CHART_VERSION_LATEST_RELEASE,
        release_name,
        "test-secret=values.yaml",
    );
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    let deployments: Api<Deployment> = Api::namespaced(k8s_env.client.clone(), &ac_namespace);
    let config_maps: Api<ConfigMap> = Api::namespaced(k8s_env.client.clone(), &ac_namespace);
    let secrets: Api<Secret> = Api::namespaced(k8s_env.client.clone(), &ac_namespace);

    let deployment_name = format!("{}-agent-control-deploy", release_name);
    retry(10, Duration::from_secs(1), || {
        // We set "nameOverride" in the secret values to force the deployment name
        // to be equal to the release name. This avoids breaking the test if the
        // default value changes in the chart.
        let _ = block_on(deployments.get(&deployment_name))?;
        Ok(())
    });
    retry(10, Duration::from_secs(1), || {
        let _ = block_on(config_maps.get("local-data-nrdot"))?;
        Ok(())
    });
    retry(10, Duration::from_secs(1), || {
        let _ = block_on(secrets.get("values-nrdot"))?;
        Ok(())
    });

    let mut cmd = ac_uninstall_cmd(&ac_namespace, &subagents_namespace, release_name);
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    let _ =
        block_on(deployments.get(&deployment_name)).expect_err("AC deployment should be deleted");
    let _ = block_on(config_maps.get("local-data-nrdot"))
        .expect_err("SubAgent config_map should be deleted");
    let _ = block_on(secrets.get("values-nrdot")).expect_err("SubAgent secret should be deleted");
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_uninstall_agent_control_clean_empty_cluster() {
    let mut k8s_env = block_on(K8sEnv::new());
    let ac_namespace = block_on(k8s_env.test_namespace());
    let subagents_namespace = block_on(k8s_env.test_namespace());

    let release_name = "uninstall-ac-clean-empty-cluster";
    let mut cmd = ac_uninstall_cmd(&ac_namespace, &subagents_namespace, release_name);
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    let mut cmd = ac_uninstall_cmd(&ac_namespace, &subagents_namespace, release_name);
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();
}

/// Builds an uninstallation command for testing purposes with a curated set of defaults and the provided arguments.
fn ac_uninstall_cmd(namespace: &str, namespace_agents: &str, release_name: &str) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("uninstall-agent-control");
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--namespace-agents").arg(namespace_agents);
    cmd.arg("--release-name").arg(release_name);
    cmd
}
