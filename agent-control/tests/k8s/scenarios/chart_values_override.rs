use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{create_config_map, start_agent_control};
use crate::k8s::tools::config::K8sAgentControlConfigBuilder;
use crate::k8s::tools::k8s_api::check_daemonset_container_env;
use crate::k8s::tools::k8s_env::K8sEnv;
use std::time::Duration;
use tempfile::tempdir;

// Pinned newrelic-logging chart version. Only relies on the DaemonSet exposing
// CLUSTER_NAME resolved from global.cluster (a stable chart contract).
const NEWRELIC_LOGGING_CHART_VERSION: &str = "1.41.0";
const AGENT_TYPE_ID: &str = "newrelic/io.fluentbit:0.1.0";

struct Case {
    agent_id: &'static str,
    local_config: String,
    expected_cluster: &'static str,
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_fluentbit_chart_values_global_precedence() {
    const DEFAULT_CLUSTER: &str = "default-from-env";

    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // Env vars consumed by the embedded fluentbit agent type's default.yaml.
    // NR_CLUSTER_NAME is the baseline that both secrets can override.
    unsafe {
        std::env::set_var("NR_LICENSE_KEY", "abcd1234");
        std::env::set_var("NR_CLUSTER_NAME", DEFAULT_CLUSTER);
        std::env::set_var("NR_STAGING", "false");
        std::env::set_var("NR_LOW_DATA_MODE", "false");
    }

    // Old way to configure still wins to avoid breaking changes
    let cases = [
        Case {
            agent_id: "fb-new-wins",
            local_config: format!(
                r#"chart_version: "{NEWRELIC_LOGGING_CHART_VERSION}"
chart_values:
  global:
    cluster: "value-from-deprecated"
  newrelic-logging:
    global:
      cluster: "value-from-new"
    rbac:
      create: false
"#
            ),
            expected_cluster: "value-from-deprecated",
        },
        Case {
            agent_id: "fb-depr-wins",
            local_config: format!(
                r#"chart_version: "{NEWRELIC_LOGGING_CHART_VERSION}"
chart_values:
  newrelic-logging:
    global:
      cluster: "value-from-new"
    rbac:
      create: false
"#
            ),
            expected_cluster: "value-from-new",
        },
        Case {
            agent_id: "fb-default-wins",
            local_config: format!(
                r#"chart_version: "{NEWRELIC_LOGGING_CHART_VERSION}"
chart_values:
  newrelic-logging:
    rbac:
      create: false
"#
            ),
            expected_cluster: DEFAULT_CLUSTER,
        },
    ];

    let mut builder = K8sAgentControlConfigBuilder::new(&ac_ns).with_namespace_agents(&agents_ns);
    for case in &cases {
        builder = builder.with_agent(case.agent_id, AGENT_TYPE_ID);
    }
    builder.write(k8s.client.clone(), tmp_dir.path());

    for case in &cases {
        block_on(create_config_map(
            k8s.client.clone(),
            &ac_ns,
            &format!("local-data-{}", case.agent_id),
            case.local_config.clone(),
        ));
    }

    let _sa = start_agent_control(k8s.client.clone(), &ac_ns, tmp_dir.path());

    // Release name = agent id, chart name = "newrelic-logging". Since they differ,
    // newrelic-logging.fullname renders "<release>-<chart>". Container name is
    // newrelic-logging.name = "newrelic-logging".
    retry(120, Duration::from_secs(1), || {
        for case in &cases {
            block_on(check_daemonset_container_env(
                k8s.client.clone(),
                agents_ns.as_str(),
                &format!("{}-newrelic-logging", case.agent_id),
                "newrelic-logging",
                "CLUSTER_NAME",
                case.expected_cluster,
            ))?;
        }
        Ok(())
    });

    unsafe {
        std::env::remove_var("NR_LICENSE_KEY");
        std::env::remove_var("NR_CLUSTER_NAME");
        std::env::remove_var("NR_STAGING");
        std::env::remove_var("NR_LOW_DATA_MODE");
    }
}
