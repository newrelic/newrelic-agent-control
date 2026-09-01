use crate::common::{
    retry::retry,
    runtime::{block_on, tokio_runtime},
};
use crate::k8s::tools::{
    agent_control::{create_config_map, start_agent_control},
    config::K8sAgentControlConfigBuilder,
    custom_agent_type::K8sCustomAgentTypeBuilder,
    instance_id,
    k8s_api::{check_helmrelease_condition, check_helmrelease_has_annotation},
    k8s_env::K8sEnv,
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::k8s::annotations::FLUX_RECONCILE_ANNOTATION_KEY;
use std::time::Duration;
use tempfile::tempdir;

/// Verifies that AC forces a Flux reconciliation on a stalled HelmRelease when a new remote
/// config is received.
///
/// Step 1: AC starts with broken `chart_values` setting `service.port` to a non-integer string.
/// Helm renders the template successfully but the k8s API rejects the Service manifest because
/// `port` must be an integer. With `remediation.retries: 1`, Flux exhausts retries and marks
/// the HelmRelease as Stalled.
///
/// Step 2: a corrected remote config (valid annotation key with one slash) is delivered via
/// OpAMP. AC detects the Stalled condition, patches `reconcile.fluxcd.io/requestedAt`, and
/// Flux reconciles the HelmRelease to Ready.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_stalled_helm_release_recovers_after_remote_config_fix() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // Agent type with configurable chart_values and remediation.retries: 1 so Flux stalls
    // quickly after a single failed install attempt.
    let agent_type_id = K8sCustomAgentTypeBuilder::empty()
        .with_agent_type_id("newrelic/com.newrelic.stall_test:0.0.1")
        .with_variables(
            r#"
chart_values:
  description: "chart values"
  type: yaml
  required: false
  default: {}
"#,
        )
        .with_objects(Some(
            r#"
repository:
  apiVersion: source.toolkit.fluxcd.io/v1
  kind: HelmRepository
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    interval: 99m
    url: https://helm.github.io/examples
    provider: generic
release:
  apiVersion: helm.toolkit.fluxcd.io/v2
  kind: HelmRelease
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    interval: 10s
    chart:
      spec:
        chart: hello-world
        version: 0.1.0
        sourceRef:
          kind: HelmRepository
          name: ${nr-sub:agent_id}
          namespace: ${nr-ac:namespace}
        reconcileStrategy: ChartVersion
        interval: 3m
    install:
      remediation:
        retries: 1
    values:
      ${nr-var:chart_values}
"#,
        ))
        .write(tmp_dir.path());

    K8sAgentControlConfigBuilder::new(&ac_ns)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_namespace_agents(&agents_ns)
        .with_agent("stall-agent", agent_type_id)
        .write(k8s.client.clone(), tmp_dir.path());

    // Broken config: service.port must be an integer; passing a string causes the k8s API to
    // reject the rendered Service manifest, making the Helm install fail.
    block_on(create_config_map(
        k8s.client.clone(),
        &ac_ns,
        "local-data-stall-agent",
        "chart_values:\n  service:\n    port: \"invalid\"\n".to_string(),
    ));

    let _ac = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::try_from("stall-agent").unwrap(),
    );

    // Wait for Flux to exhaust the single install retry and mark the HelmRelease as Stalled.
    retry(120, Duration::from_secs(5), || {
        block_on(check_helmrelease_condition(
            k8s.client.clone(),
            &ac_ns,
            "stall-agent",
            "Stalled",
            "True",
        ))
    });

    // Deliver a corrected remote config with a valid service port.
    server.set_config_response(
        sub_agent_instance_id,
        "chart_values:\n  service:\n    port: 80\n",
    );

    // AC should detect the Stalled condition and patch reconcile.fluxcd.io/requestedAt on the
    // HelmRelease CRD to force Flux out of its frozen state.
    retry(60, Duration::from_secs(2), || {
        block_on(check_helmrelease_has_annotation(
            k8s.client.clone(),
            &ac_ns,
            "stall-agent",
            FLUX_RECONCILE_ANNOTATION_KEY,
        ))
    });

    // Flux reconciles with the corrected service port and the HelmRelease becomes Ready.
    retry(120, Duration::from_secs(5), || {
        block_on(check_helmrelease_condition(
            k8s.client.clone(),
            &ac_ns,
            "stall-agent",
            "Ready",
            "True",
        ))
    });
}
