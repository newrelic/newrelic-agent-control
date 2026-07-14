use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{create_config_map, start_agent_control};
use crate::k8s::tools::config::K8sAgentControlConfigBuilder;
use crate::k8s::tools::custom_agent_type::K8sCustomAgentType;
use crate::k8s::tools::k8s_api::{
    check_helmrelease_labels_contains, check_helmrelease_spec_values, create_values_secret,
};
use crate::k8s::tools::k8s_env::K8sEnv;
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_template_secrets() {
    let test_name = "k8s_template_secrets";

    let mut k8s = block_on(K8sEnv::new());
    k8s.port_forward("vault-0", 8200, 8200);
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agents = r#"
  hello-world:
    agent_type: "newrelic/com.newrelic.custom_agent:0.0.1"
"#;

    let secrets_providers = r#"  vault:
    sources:
      sourceA:
        url: http://127.0.0.1:8200/v1/
        token: root
        engine: kv1
      sourceB:
        url: http://127.0.0.1:8200/v1/
        token: root
        engine: kv2"#;

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_agents(agents)
        .with_secrets_providers(secrets_providers)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &namespace,
        "local-data-hello-world",
        format!(
            r#"chart_values:
  hashicorpVaultV1Key: ${{nr-vault:sourceA:kv-v1:my-secret:foo1}}
  hashicorpVaultV2Key: ${{nr-vault:sourceB:secret:my-secret:foo2}}
  k8sSecretKey: ${{nr-kubesec:{namespace}:pod-secrets:foo3}}
  envVarKey: ${{nr-env:{test_name}_foo4}}"#
        ),
    ));

    K8sCustomAgentType::empty()
        .with_variables(
            r#"
chart_values:
  description: "chart_values"
  type: yaml
  required: false
  default: { }
"#,
        )
        .with_objects(Some(
            r#"
release:
  apiVersion: helm.toolkit.fluxcd.io/v2
  kind: HelmRelease
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
    labels:
      agentTypeEnvVarKey: ${nr-env:k8s_template_secrets_zip4}
  spec:
    # we don't want to trigger this in the test to avoid extra load in the cluster
    interval: 10s
    releaseName: ${nr-sub:agent_id}
    targetNamespace: ${nr-ac:namespace_agents}
    chart:
      spec:
        chart: hello-world
        version: "0.1.0"
        sourceRef:
          kind: HelmRepository
          name: ${nr-sub:agent_id}
          namespace: ${nr-ac:namespace}
    values:
      ${nr-var:chart_values}
"#,
        ))
        .build(tmp_dir.path());
    let _sa = start_agent_control(k8s.client.clone(), &namespace, tmp_dir.path());

    // Now, we create all the required secrets.
    // Hashicorp Vault secrets -> handled in the Tiltfile.

    // K8s secrets -> created here on demand.
    let name = "pod-secrets";
    let key = "foo3";
    let value = "bar3";
    create_values_secret(k8s.client.clone(), &namespace, name, key, value.to_string());

    // env var secrets -> created here on demand.
    unsafe {
        std::env::set_var(format!("{test_name}_foo4"), "bar4");
        std::env::set_var(format!("{test_name}_zip4"), "zap4");
    }

    // Check the HelmRelease is created with the secrets correctly populated
    let expected_spec_values = r#"
hashicorpVaultV1Key: bar1
hashicorpVaultV2Key: bar2
k8sSecretKey: bar3
envVarKey: bar4
    "#;

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_labels = Some(BTreeMap::from_iter(vec![(
            "agentTypeEnvVarKey".to_string(),
            "zap4".to_string(),
        )]));
        block_on(check_helmrelease_labels_contains(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_labels,
        ))
    });

    unsafe {
        std::env::remove_var(format!("{test_name}_foo4"));
        std::env::remove_var(format!("{test_name}_zip4"));
    }
}
