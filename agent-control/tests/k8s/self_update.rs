use super::tools::k8s_api::create_values_secret;
use super::tools::k8s_env::K8sEnv;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::{check_latest_health_status, check_latest_health_status_was_healthy};
use crate::common::opamp::FakeServer;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::instance_id;
use crate::k8s::tools::local_chart::{LOCAL_CHART_REPOSITORY, agent_control_deploymet::*};
use crate::k8s::tools::logs::{AC_LABEL_SELECTOR, print_pod_logs};
use crate::k8s::tools::opamp::get_minikube_opamp_url_from_fake_server;
use assert_cmd::Command;
use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::{Api, Client};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AnyValue, KeyValue, RemoteConfigStatuses};
use std::thread::sleep;
use std::time::Duration;
use url::Url;

const POLL_INTERVAL: u64 = 5;

const SECRET_NAME: &str = "ac-values";
const VALUES_KEY: &str = "values.yaml";

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the image from our public repository,
/// then sends a RemoteConfig with an AC chart version update to the local build image.
fn k8s_self_update_bump_chart_version_from_last_release_to_local_new_config() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_LATEST_RELEASE,
    );

    let ac_config = format!(
        r#"
agents: {{}}
chart_version: {CHART_VERSION_DEV_1}
"#
    );

    opamp_server.set_config_response(ac_instance_id.clone(), ac_config.as_str());

    // Assert that opamp server receives Agent description with updated version.
    // Also the rest of the config with the new agent has been effectevely applied.
    retry(60, Duration::from_secs(5), || {
        let current_attributes = opamp_server
            .get_attributes(&ac_instance_id)
            .ok_or_else(|| "Identifying attributes not found".to_string())?;

        if !current_attributes
            .identifying_attributes
            .contains(&KeyValue {
                key: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(CHART_VERSION_DEV_1.to_string())),
                }),
            })
        {
            return Err(
                format!("new version has not been reported: {current_attributes:?}").into(),
            );
        }

        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_instance_id,
            ac_config.clone(),
        )?;
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// and asserts that the new AC version is sending OpAmp messages.
fn k8s_self_update_bump_chart_version() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_DEV_1,
    );

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents: {{}}
chart_version: {CHART_VERSION_DEV_2}
"#
        ),
    );

    // Assert that opamp server receives Agent description with updated version.
    retry(60, Duration::from_secs(5), || {
        let current_attributes = opamp_server
            .get_attributes(&ac_instance_id)
            .ok_or_else(|| "Identifying attributes not found".to_string())?;

        if !current_attributes
            .identifying_attributes
            .contains(&KeyValue {
                key: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(CHART_VERSION_DEV_2.to_string())),
                }),
            })
        {
            return Err(
                format!("new version has not been reported: {current_attributes:?}").into(),
            );
        }

        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// also introducing a change in the AC config which should be applied to the new AC version.
fn k8s_self_update_bump_chart_version_with_new_config() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_DEV_1,
    );

    // This agent will not actually be deployed since misses the chart_version config.
    let agents_config = r#"agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.1.0
"#;

    let ac_config = format!(
        r#"
{agents_config}
chart_version: {CHART_VERSION_DEV_2}
"#
    );

    opamp_server.set_config_response(ac_instance_id.clone(), ac_config.as_str());

    // Assert that opamp server receives Agent description with updated version.
    // Also the rest of the config with the new agent has been effectevely applied.
    retry(60, Duration::from_secs(5), || {
        let current_attributes = opamp_server
            .get_attributes(&ac_instance_id)
            .ok_or_else(|| "Identifying attributes not found".to_string())?;

        if !current_attributes
            .identifying_attributes
            .contains(&KeyValue {
                key: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(CHART_VERSION_DEV_2.to_string())),
                }),
            })
        {
            return Err(
                format!("new version has not been reported: {current_attributes:?}").into(),
            );
        }

        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )?;
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_instance_id,
            ac_config.clone(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// pointing to a version that doesn't exist. It expects that current AC keeps working and reports
/// unhealthy status then a new correct remote config arrives, it's applied and the new ac reports healthy.
fn k8s_self_update_new_version_fails_to_start_next_receives_correct_version() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_DEV_1,
    );

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents: {{}}
chart_version: {MISSING_VERSION}
"#
        ),
    );
    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32, // The configuration was Applied even if it led to an unhealthy AC.
        )?;
        check_latest_health_status(&opamp_server, &ac_instance_id, |status| {
            !status.healthy
                && status.last_error.contains(
                    &format!("no 'agent-control-deployment' chart with version matching '{MISSING_VERSION}' found"),
                )
        })
    });

    let ac_config = format!(
        r#"
agents: {{}}
chart_version: {CHART_VERSION_DEV_2}
"#
    );

    opamp_server.set_config_response(ac_instance_id.clone(), ac_config.as_str());

    retry(60, Duration::from_secs(5), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_instance_id,
            ac_config.clone(),
        )?;
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;

        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// pointing to a version that contains an image. It expects that current AC keeps working and reports
/// unhealthy status.
fn k8s_self_update_new_version_failing_image() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(
        k8s.client.clone(),
        &opamp_server,
        &namespace,
        CHART_VERSION_DEV_2,
    );

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents: {{}}
chart_version: {CHART_VERSION_CRASHLOOP}
"#
        ),
    );

    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32, // The configuration was Applied even if it led to an unhealthy AC.
        )?;

        // Select a pod with the label set by tilt for the crashing pod
        let pods: Api<Pod> = Api::namespaced(k8s.client.clone(), &namespace);
        let lp = ListParams::default().labels("app=failing-pod");
        let pod_list = block_on(pods.list(&lp))?;

        // Iterate over the Pods matching the label to ensure are crashing.
        let mut pod_crashing = false;
        'pods_loop: for p in pod_list.iter() {
            if p.status
                .as_ref()
                .map(|status| &status.container_statuses)
                .into_iter()
                .flat_map(|statuses| statuses.iter().flat_map(|status_vec| status_vec.iter()))
                .filter_map(|container_status| container_status.clone().state)
                .filter_map(|state| state.waiting)
                .any(|waiting| waiting.reason == Some("CrashLoopBackOff".to_string()))
            {
                pod_crashing = true;
                break 'pods_loop;
            }
        }
        if !pod_crashing {
            return Err("No new Pod crashing found".into());
        }

        check_latest_health_status(&opamp_server, &ac_instance_id, |status| {
            !status.healthy && status.last_error.contains("has 1 unavailable replicas")
        })
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
        "config": {
          // Disable the SI creation
          "fleet_control": {
            "enabled": false,
          },
          "acRemoteUpdate": true,
          "cdRemoteUpdate": false,
          "override": {
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
          }
        },
        "global": {
          "cluster": "test-cluster",
          "licenseKey": "***",
        },
    })
    .to_string()
}
