use crate::common::{
    health::{check_latest_health_status, check_latest_health_status_was_healthy},
    remote_config_status::check_latest_remote_config_status_is_expected,
    retry::{retry, retry_never},
    runtime::{block_on, tokio_runtime},
};
use fake_opamp_server::FakeServer;

use crate::k8s::tools::{
    agent_control::{create_config_map, start_agent_control},
    config::K8sAgentControlConfigBuilder,
    custom_agent_type::K8sCustomAgentTypeBuilder,
    instance_id,
    k8s_env::K8sEnv,
};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{Api, Client, api::PostParams};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::tempdir;

/// Given AC with a sub-agent whose health checks reference workloads by name (`kind: Deployment`),
/// verify that health is correctly reported as healthy once the target Deployment is present.
///
/// The agent type also defines StatefulSet and DaemonSet checks to verify that absent workloads
/// do not affect the aggregate result.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_direct_workload_health_checks() {
    let server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // Create the deployment that the health check will monitor by name.
    // Using 0 replicas: no pods are scheduled, so the deployment is immediately healthy
    // (0 available == 0 desired, no unavailable), without requiring an image pull.
    block_on(create_deployment(
        k8s.client.clone(),
        &agents_ns,
        "hello-world",
        0,
    ));

    let agent_type_id = direct_checks_agent_type().write(tmp_dir.path());

    K8sAgentControlConfigBuilder::new(&ac_ns)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_namespace_agents(&agents_ns)
        .with_agent("hello-world", agent_type_id)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &ac_ns,
        "local-data-hello-world",
        "{}".to_string(),
    ));

    let _ac = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&server, &sub_agent_instance_id.clone())
    });
}

/// Given AC with a sub-agent whose health checks reference a Deployment by name, verify that
/// health is reported as unhealthy when the Deployment has desired replicas that are not available.
///
/// The Deployment uses `imagePullPolicy: Never` with a non-existent image, so pods can never
/// start and `available_replicas` remains permanently below `desired_replicas`.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_direct_workload_health_checks_unhealthy() {
    let server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // Create a deployment with replicas=1 but an image that can never be pulled locally.
    // available_replicas (0) < desired_replicas (1) → health check reports Unhealthy.
    block_on(create_deployment(
        k8s.client.clone(),
        &agents_ns,
        "hello-world",
        1,
    ));

    let agent_type_id = direct_checks_agent_type().write(tmp_dir.path());

    K8sAgentControlConfigBuilder::new(&ac_ns)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_namespace_agents(&agents_ns)
        .with_agent("hello-world", agent_type_id)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &ac_ns,
        "local-data-hello-world",
        "{}".to_string(),
    ));

    let _ac = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status(&server, &sub_agent_instance_id.clone(), |s| !s.healthy)
    });
}

/// Given a k8s sub-agent whose agent type declares no `health:` block, but whose initial
/// supervisor fails to build because a required variable is missing from its config, no
/// `ComponentHealth` should be reported for it via OpAMP.
///
/// `report_unhealthy_from_error` is a second producer of health events besides the
/// health-checker thread: supervisor build/apply/start failures report unhealthy regardless of
/// whether a `health:` block is configured, and that report must be suppressed too.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_no_health_in_agent_type_when_start_failure() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agent_type_id = K8sCustomAgentTypeBuilder::empty()
        .with_health(None)
        .with_variables(
            r#"
fake_var:
  description: "Required variable missing from config"
  type: "string"
  required: true
"#,
        )
        .write(tmp_dir.path());

    K8sAgentControlConfigBuilder::new(&ac_ns)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_namespace_agents(&agents_ns)
        .with_agent("hello-world", agent_type_id)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &ac_ns,
        "local-data-hello-world",
        "some: broken config".to_string(),
    ));

    let _ac = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("hello-world").unwrap(),
    );

    server.set_config_response(sub_agent_instance_id.clone(), "some: broken config");

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )
    });

    // No health status is expected, despite the supervisor start failure.
    retry_never(10, Duration::from_secs(1), || {
        match server.get_health_status(sub_agent_instance_id.clone()) {
            None => Ok(()),
            Some(health) => Err(format!(
                "Expected no ComponentHealth for sub-agent without `health:` in agent type, got: {health:?}"
            )
            .into()),
        }
    });
}

/// Given a k8s sub-agent whose agent type declares a `health:` block, health should be reported
/// via OpAMP if its initial supervisor fails to build, same as the case above but with health
/// enabled.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_health_in_agent_type_when_start_failure() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agent_type_id = K8sCustomAgentTypeBuilder::empty()
        .with_variables(
            r#"
fake_var:
  description: "Required variable missing from config"
  type: "string"
  required: true
"#,
        )
        .with_health(Some(
            r#"{"interval": "1s", "initial_delay": "2s", "checks": [{ "namespace": "${nr-ac:namespace_agents}", "name": "${nr-sub:agent_id}", "kind": "Deployment"}]}"#,
        ))
        .write(tmp_dir.path());

    K8sAgentControlConfigBuilder::new(&ac_ns)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_namespace_agents(&agents_ns)
        .with_agent("hello-world", agent_type_id)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &ac_ns,
        "local-data-hello-world",
        "some: broken config".to_string(),
    ));

    let _ac = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("hello-world").unwrap(),
    );

    server.set_config_response(sub_agent_instance_id.clone(), "some: broken config");

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;
        match server.get_health_status(sub_agent_instance_id.clone()) {
            Some(_) => Ok(()),
            None => Err("Expected ComponentHealth for sub-agent".into()),
        }
    });
}

/// The integration test defines a Deployment. StatefulSet and DaemonSet are also defined to check that the
/// health-checker is healthy when one of them is found and healthy, allowing the definition of health-checks
/// for Agent-Type whose workload definition is configurable.
///
/// No objects, the workload is defined externally
fn direct_checks_agent_type() -> K8sCustomAgentTypeBuilder {
    K8sCustomAgentTypeBuilder::empty().with_health(Some(
        r#"
interval: 5s
initial_delay: 2s
checks:
  - namespace: ${nr-ac:namespace_agents}
    name: ${nr-sub:agent_id}
    kind: Deployment
  - namespace: ${nr-ac:namespace_agents}
    name: ${nr-sub:agent_id}
    kind: StatefulSet
  - namespace: ${nr-ac:namespace_agents}
    name: ${nr-sub:agent_id}
    kind: DaemonSet
"#,
    ))
}

/// Creates a Deployment named `name` in `namespace` with the given replica count.
///
/// `imagePullPolicy: Never` with a placeholder image is used throughout so no image pull
/// is ever attempted, regardless of replica count. When `replicas` is 0, the deployment is
/// immediately healthy (0 available == 0 desired). When `replicas` is > 0, pods can never
/// start, keeping `available_replicas` permanently below `desired_replicas`.
async fn create_deployment(client: Client, namespace: &str, name: &str, replicas: i32) {
    let labels = BTreeMap::from([("app".to_string(), name.to_string())]);
    let container = Container {
        name: "main".to_string(),
        image: Some("nonexistent-image:latest".to_string()),
        image_pull_policy: Some("Never".to_string()),
        ..Default::default()
    };
    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    api.create(&PostParams::default(), &deployment)
        .await
        .expect("failed to create deployment");
}
