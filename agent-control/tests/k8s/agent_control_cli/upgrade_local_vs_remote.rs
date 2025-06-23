use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::agent_control_cli::installation::{ac_install_cmd, create_simple_values_secret};
use crate::k8s::self_update::{LOCAL_CHART_NEW_VERSION, LOCAL_CHART_PREVIOUS_VERSION};
use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::config::{
    AgentControlDynamicConfig, helmrelease_v2_type_meta,
};
use newrelic_agent_control::agent_control::version_updater::k8s::K8sACUpdater;
use newrelic_agent_control::agent_control::version_updater::updater::VersionUpdater;
use newrelic_agent_control::cli::install_agent_control::RELEASE_NAME;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL};
use newrelic_agent_control::sub_agent::version::version_checker::VersionCheckError;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

#[test]
#[ignore = "needs k8s cluster"]
// This test can break if the chart introduces any breaking changes.
// If this situation occurs, we will need to disable the test or use
// a similar workaround than the one we use in the tiltfile.
// The test is checking how local and remote upgrade are interacting
fn k8s_cli_local_and_remote_updates() {
    let mut k8s_env = block_on(K8sEnv::new());
    let namespace = block_on(k8s_env.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), namespace.clone()).unwrap());

    create_simple_values_secret(
        k8s_env.client.clone(),
        &namespace,
        "test-secret",
        "values.yaml",
    );

    // running installer first time
    let mut cmd = ac_install_cmd(
        &namespace,
        LOCAL_CHART_PREVIOUS_VERSION,
        "test-secret=values.yaml",
    );
    cmd.assert().success();

    retry(15, Duration::from_secs(5), || {
        check_version_and_source(&k8s_client, LOCAL_CHART_PREVIOUS_VERSION, LOCAL_VAL)
    });

    // running installer second time and doing an upgrade
    let mut cmd = ac_install_cmd(
        &namespace,
        LOCAL_CHART_NEW_VERSION,
        "test-secret=values.yaml",
    );
    cmd.assert().success();

    retry(15, Duration::from_secs(5), || {
        check_version_and_source(&k8s_client, LOCAL_CHART_NEW_VERSION, LOCAL_VAL)
    });

    // running updater doing an upgrade to "*"
    let updater = K8sACUpdater::new(k8s_client.clone(), LOCAL_CHART_NEW_VERSION.to_string());
    let latest_version = "*";
    let config_to_update = &AgentControlDynamicConfig {
        agents: Default::default(),
        chart_version: Some(latest_version.to_string()),
    };
    updater
        .update(config_to_update)
        .expect("updater should not fail");

    retry(15, Duration::from_secs(5), || {
        check_version_and_source(&k8s_client, latest_version, REMOTE_VAL)
    });

    // running another local update does not change the version, but it updates anyway the helmRelease object
    let mut cmd = ac_install_cmd(
        &namespace,
        LOCAL_CHART_PREVIOUS_VERSION,
        "test-secret=values.yaml",
    );
    cmd.arg("--extra-labels").arg("env=testing");
    cmd.assert().success();

    retry(15, Duration::from_secs(5), || {
        check_version_and_source(&k8s_client, latest_version, REMOTE_VAL)?;

        let obj = k8s_client
            .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)?
            .ok_or(VersionCheckError::Generic(format!(
                "helmRelease object not found: {RELEASE_NAME}",
            )))?;

        // Notice that the extra label is set by the installer despite the fact that the version is not changed.
        if "testing"
            != obj
                .metadata
                .clone()
                .labels
                .unwrap_or_default()
                .get("env")
                .unwrap()
        {
            return Err(format!("label was not added: {obj:?}").into());
        }
        Ok(())
    });
}

pub fn check_version_and_source(
    k8s_client: &SyncK8sClient,
    version: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    let obj = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)?
        .ok_or(VersionCheckError::Generic(format!(
            "helmRelease object not found: {RELEASE_NAME}",
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
            .get(AGENT_CONTROL_VERSION_SET_FROM)
            .unwrap()
    {
        return Err(format!("HelmRelease source not correct: {source}, {obj:?}").into());
    }
    Ok(())
}
