// use crate::common::opamp::FakeServer;
// use crate::common::retry::retry;
// use crate::common::runtime::{block_on, tokio_runtime};
// use crate::k8s::agent_control_cli::installation::{ac_install_cmd, create_simple_values_secret};
// use crate::k8s::tools::cmd::print_cli_output;
// use crate::k8s::tools::instance_id;
// use crate::k8s::tools::k8s_env::K8sEnv;
// use crate::k8s::tools::local_chart::agent_control_deploymet::{
//     CHART_VERSION_DEV_1, CHART_VERSION_DEV_2, CHART_VERSION_LATEST_RELEASE,
// };
// use crate::k8s::tools::logs::print_pod_logs;
// use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use newrelic_agent_control::k8s::client::SyncK8sClient;
// use newrelic_agent_control::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL};
use newrelic_agent_control::version_checker::VersionCheckError;
use std::error::Error;
// use std::sync::Arc;
// use std::time::Duration;
// /// TODO: Re-enable this test after PR #1752 (this repo) and helm-charts PR #1965 are merged to main.
// const CLI_AC_LABEL_SELECTOR: &str = "app.kubernetes.io/name=agent-control-deployment";
//
// #[test]
// #[ignore = "needs k8s cluster"]
// // This test can break if the chart introduces any breaking changes.
// // If this situation occurs, we will need to disable the test or use
// // a similar workaround than the one we use in the tiltfile.
// // The test is checking how local and remote upgrade are interacting
// fn k8s_cli_local_and_remote_updates() {
//     let mut opamp_server = FakeServer::start_new();
//     let mut k8s_env = block_on(K8sEnv::new());
//     let ac_namespace = block_on(k8s_env.test_namespace());
//     let subagents_namespace = block_on(k8s_env.test_namespace());
//     let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
//
//     print_pod_logs(k8s_env.client.clone(), &ac_namespace, CLI_AC_LABEL_SELECTOR);
//
//     create_simple_values_secret(
//         k8s_env.client.clone(),
//         &ac_namespace,
//         &subagents_namespace,
//         "test-secret",
//         opamp_server.endpoint().as_str(),
//         "values.yaml",
//     );
//
//     let release_name = "local-and-remote-updates";
//
//     // running installer first time
//     let mut cmd = ac_install_cmd(
//         &ac_namespace,
//         CHART_VERSION_DEV_1,
//         release_name,
//         "test-secret=values.yaml",
//     );
//
//     let assert = cmd.assert();
//     print_cli_output(&assert);
//     assert.success();
//
//     retry(15, Duration::from_secs(5), || {
//         check_version_and_source(
//             &k8s_client,
//             CHART_VERSION_DEV_1,
//             LOCAL_VAL,
//             &ac_namespace,
//             release_name,
//             AGENT_CONTROL_VERSION_SET_FROM,
//         )
//     });
//
//     // running installer second time and doing an upgrade
//     let mut cmd = ac_install_cmd(
//         &ac_namespace,
//         CHART_VERSION_DEV_2,
//         release_name,
//         "test-secret=values.yaml",
//     );
//     let assert = cmd.assert();
//     print_cli_output(&assert);
//     assert.success();
//
//     retry(15, Duration::from_secs(5), || {
//         check_version_and_source(
//             &k8s_client,
//             CHART_VERSION_DEV_2,
//             LOCAL_VAL,
//             &ac_namespace,
//             release_name,
//             AGENT_CONTROL_VERSION_SET_FROM,
//         )
//     });
//
//     let ac_instance_id = instance_id::get_instance_id(
//         k8s_env.client.clone(),
//         ac_namespace.as_str(),
//         &AgentID::AgentControl,
//     );
//     opamp_server.set_config_response(
//         ac_instance_id.clone(),
//         format!(
//             r#"
// agents: {{}}
// chart_version: "{CHART_VERSION_LATEST_RELEASE}"
// "#
//         ),
//     );
//
//     retry(15, Duration::from_secs(5), || {
//         check_version_and_source(
//             &k8s_client,
//             CHART_VERSION_LATEST_RELEASE,
//             REMOTE_VAL,
//             &ac_namespace,
//             release_name,
//             AGENT_CONTROL_VERSION_SET_FROM,
//         )
//     });
//
//     // running another local update does not change the version, but it updates anyway the helmRelease object
//     let mut cmd = ac_install_cmd(
//         &ac_namespace,
//         CHART_VERSION_DEV_1,
//         release_name,
//         "test-secret=values.yaml",
//     );
//     cmd.arg("--extra-labels").arg("env=testing");
//     let assert = cmd.assert();
//     print_cli_output(&assert);
//     assert.success();
//
//     retry(15, Duration::from_secs(5), || {
//         check_version_and_source(
//             &k8s_client,
//             CHART_VERSION_LATEST_RELEASE,
//             REMOTE_VAL,
//             &ac_namespace,
//             release_name,
//             AGENT_CONTROL_VERSION_SET_FROM,
//         )?;
//
//         let obj = k8s_client
//             .get_dynamic_object(&helmrelease_v2_type_meta(), release_name, &ac_namespace)?
//             .ok_or(VersionCheckError(format!(
//                 "helmRelease object not found: {release_name}",
//             )))?;
//
//         // Notice that the extra label is set by the installer despite the fact that the version is not changed.
//         if "testing"
//             != obj
//                 .metadata
//                 .clone()
//                 .labels
//                 .unwrap_or_default()
//                 .get("env")
//                 .unwrap()
//         {
//             return Err(format!("label was not added: {obj:?}").into());
//         }
//         Ok(())
//     });
// }

pub fn check_version_and_source(
    k8s_client: &SyncK8sClient,
    version: &str,
    source: &str,
    namespace: &str,
    release_name: &str,
    main_label: &str,
) -> Result<(), Box<dyn Error>> {
    let obj = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name, namespace)?
        .ok_or(VersionCheckError(format!(
            "helmRelease object not found: {release_name}",
        )))?;

    if version
        != obj
            .data
            .get("spec")
            .unwrap()
            .clone()
            .get("chart")
            .unwrap()
            .get("spec")
            .unwrap()
            .get("version")
            .unwrap()
            .as_str()
            .unwrap()
    {
        return Err(format!("HelmRelease version not correct: {version},  {obj:?}").into());
    }

    if source
        != obj
            .metadata
            .clone()
            .labels
            .unwrap_or_default()
            .get(main_label)
            .unwrap()
    {
        return Err(format!("HelmRelease source not correct: {source}, {obj:?}").into());
    }
    Ok(())
}
