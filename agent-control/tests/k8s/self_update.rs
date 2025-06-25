use super::tools::k8s_api::create_values_secret;
use super::tools::k8s_env::K8sEnv;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::{check_latest_health_status, check_latest_health_status_was_healthy};
use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::instance_id;
use crate::k8s::tools::logs::{AC_LABEL_SELECTOR, print_pod_logs};
use assert_cmd::Command;
use kube::Client;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::{AnyValue, KeyValue, RemoteConfigStatuses};
use std::str::FromStr;
use std::time::Duration;
use url::Url;

// These tests leverages an in-cluster chart repository populated with fixed versions which consist in the latest
// released chart with a changed version.
// The AC image corresponds to the compiled from the current code. Tilt is used to orchestrate all these
// test environment set-up.
// TODO we might drastically reduce the execution time of these test if we hack a way to reduce the opamp poll interval

// Test environment reference values defined in Tiltfile.
const AC_DEV_IMAGE_REPO: &str = "tilt.local/ac-dev";
const AC_DEV_IMAGE_TAG: &str = "dev";
pub const LOCAL_CHART_REPOSITORY: &str = "http://chartmuseum.default.svc.cluster.local:8080";
pub const LOCAL_CHART_PREVIOUS_VERSION: &str = "0.0.1";
pub const LOCAL_CHART_NEW_VERSION: &str = "0.0.2";
const MISSING_VERSION: &str = "9.9.9";

const SECRET_NAME: &str = "ac-values";
const VALUES_KEY: &str = "values.yaml";

// URL to access to services binded on ports from minikube host
// https://minikube.sigs.k8s.io/docs/handbook/host-access/
const MINIKUBE_HOST_ACCESS: &str = "host.minikube.internal";

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// and asserts that the new AC version is sending OpAmp messages.
fn k8s_self_update_bump_chart_version() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(k8s.client.clone(), &opamp_server, &namespace);

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        ConfigResponse::from(
            format!(
                r#"
agents: {{}}
chart_version: {LOCAL_CHART_NEW_VERSION}
"#
            )
            .as_str(),
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
                key: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(LOCAL_CHART_NEW_VERSION.to_string())),
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

    let ac_instance_id = bootstrap_ac(k8s.client.clone(), &opamp_server, &namespace);

    // This agent will not actually be deployed since misses the chart_version config.
    let agents_config = r#"agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.1.0
"#;

    let ac_config = format!(
        r#"
{agents_config}
chart_version: {LOCAL_CHART_NEW_VERSION}
"#
    );

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        ConfigResponse::from(ac_config.as_str()),
    );

    // Assert that opamp server receives Agent description with updated version.
    // Also the rest of the config with the new agent has been effectevely applied.
    retry(60, Duration::from_secs(5), || {
        let current_attributes = opamp_server
            .get_attributes(&ac_instance_id)
            .ok_or_else(|| "Identifying attributes not found".to_string())?;

        if !current_attributes
            .identifying_attributes
            .contains(&KeyValue {
                key: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(LOCAL_CHART_NEW_VERSION.to_string())),
                }),
            })
        {
            return Err(format!(
                "new version has not been reported: {:?}",
                current_attributes
            )
            .into());
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
/// also comes with a fail config that should prevent the update to take place.
fn k8s_self_update_bump_chart_version_with_broken_config() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(k8s.client.clone(), &opamp_server, &namespace);

    let agents_config = r#"agents:
  fail-agent:
    agent_type: newrelic/non.existent.type:0.1.0
"#;

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        ConfigResponse::from(
            format!(
                r#"
{agents_config}
chart_version: {LOCAL_CHART_NEW_VERSION}
"#
            )
            .as_str(),
        ),
    );

    // Assert that opamp server receives Agent description with current version, that contains
    // the failing remote config status.
    retry(60, Duration::from_secs(5), || {
        let current_attributes = opamp_server
            .get_attributes(&ac_instance_id)
            .ok_or_else(|| "Identifying attributes not found".to_string())?;

        // this assert might never detect a failure update if the old version reports the following conditions.
        // TODO: since we could have false-positives, consider if this is properly covered by unit-tests and
        // garbage-collect if possible.
        if !current_attributes
            .identifying_attributes
            .contains(&KeyValue {
                key: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(LOCAL_CHART_PREVIOUS_VERSION.to_string())),
                }),
            })
        {
            return Err(format!("new version has been reported: {:?}", current_attributes).into());
        }

        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;

        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
/// This test installs AC using the CLI, then sends a RemoteConfig with an AC chart version update
/// pointing to a version that doesn't exists. It expects that current AC keeps working and reports
/// unhealthy status.
fn k8s_self_update_new_version_fails_to_start() {
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    let ac_instance_id = bootstrap_ac(k8s.client.clone(), &opamp_server, &namespace);

    // AC should start correctly and finally report healthy
    retry(60, Duration::from_secs(5), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        ConfigResponse::from(
            format!(
                r#"
agents: {{}}
chart_version: {MISSING_VERSION}
"#
            )
            .as_str(),
        ),
    );
    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32, // The configuration was Applied even if it led to an unhealthy AC.
        )?;
        check_latest_health_status(&opamp_server, &ac_instance_id, |status| {
            (!status.healthy)
                && (status
                    .last_error
                    .contains("latest generation of object has not been reconciled")) // Expected error when chart version doesn't exist
        })
    });
}

/// installs ac with cli using minimal values set, pointing it to fake opamp server and printing
/// pod logs to stdout
fn bootstrap_ac(client: Client, opamp_server: &FakeServer, namespace: &str) -> InstanceID {
    let mut opamp_endpoint = Url::from_str(&opamp_server.endpoint()).unwrap();
    opamp_endpoint.set_host(Some(MINIKUBE_HOST_ACCESS)).unwrap();

    print_pod_logs(client.clone(), namespace, AC_LABEL_SELECTOR);

    create_values_secret(
        client.clone(),
        namespace,
        SECRET_NAME,
        VALUES_KEY,
        ac_chart_values(opamp_endpoint, namespace),
    );

    install_ac_with_cli(namespace);

    instance_id::get_instance_id(client.clone(), namespace, &AgentID::new_agent_control_id())
}

fn install_ac_with_cli(namespace: &str) {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();

    cmd.arg("install-agent-control");
    cmd.arg("--log-level").arg("debug");
    cmd.arg("--repository-url").arg(LOCAL_CHART_REPOSITORY);
    cmd.arg("--chart-version").arg(LOCAL_CHART_PREVIOUS_VERSION);
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--secrets")
        .arg(format!("{SECRET_NAME}={VALUES_KEY}"));
    cmd.arg("--skip-installation-check");
    cmd.assert().success();
}

fn ac_chart_values(opamp_endpoint: Url, name_override: &str) -> String {
    serde_json::json!({
        // give an unique name per test to the cluster role to avoid collisions
        "nameOverride": name_override,
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
              "fleet_control": {
                "endpoint": opamp_endpoint.as_str(),
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
        "image": {
          "repository": AC_DEV_IMAGE_REPO,
          "tag": AC_DEV_IMAGE_TAG,
          "pullPolicy": "Never",
        },
    })
    .to_string()
}
