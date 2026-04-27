use crate::{
    common::{
        health::{check_latest_health_status, check_latest_health_status_was_healthy},
        opamp::FakeServer,
        retry::retry,
        runtime::block_on,
    },
    k8s::tools::agent_control::CUSTOM_AGENT_TYPE_DIRECT_CHECKS_PATH,
};

use crate::k8s::tools::{
    agent_control::start_agent_control_with_testdata_config, instance_id, k8s_env::K8sEnv,
};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{Api, Client, api::PostParams};
use newrelic_agent_control::agent_control::agent_id::AgentID;
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
    let test_name = "k8s_direct_workload_health_checks";

    let server = FakeServer::start_new();

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

    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_DIRECT_CHECKS_PATH,
        k8s.client.clone(),
        &ac_ns,
        &agents_ns,
        Some(&server.endpoint()),
        Some(&server.jwks_endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

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
    let test_name = "k8s_direct_workload_health_checks";

    let server = FakeServer::start_new();

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

    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_DIRECT_CHECKS_PATH,
        k8s.client.clone(),
        &ac_ns,
        &agents_ns,
        Some(&server.endpoint()),
        Some(&server.jwks_endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status(&server, &sub_agent_instance_id.clone(), |s| !s.healthy)
    });
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
