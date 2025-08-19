use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::agent_control_cli::upgrade_local_vs_remote::check_version_and_source;
use crate::k8s::tools::k8s_env::K8sEnv;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::DynamicObject;
use newrelic_agent_control::agent_control::config::{
    AgentControlDynamicConfig, helmrelease_v2_type_meta,
};
use newrelic_agent_control::agent_control::version_updater::k8s::K8sACUpdater;
use newrelic_agent_control::agent_control::version_updater::updater::VersionUpdater;
use newrelic_agent_control::cli::install::agent_control::AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME;
use newrelic_agent_control::cli::install::flux::AGENT_CONTROL_CD_RELEASE_NAME;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const CURRENT_AC_VERSION: &str = "1.2.3-beta";
const NEW_AC_VERSION: &str = "1.2.3";
const CURRENT_CD_VERSION: &str = "1.2.5-beta";
const NEW_CD_VERSION: &str = "1.2.5";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_run_updater_for_cd_and_ac() {
    // set up the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());

    let updater = K8sACUpdater::new(
        true,
        true,
        k8s_client.clone(),
        test_ns.clone(),
        CURRENT_AC_VERSION.to_string(),
        AGENT_CONTROL_CD_RELEASE_NAME.to_string(),
    );

    let config_to_update = &AgentControlDynamicConfig {
        agents: Default::default(),
        chart_version: Some(NEW_AC_VERSION.to_string()),
        cd_chart_version: Some(NEW_CD_VERSION.to_string()),
    };

    let ac_dynamic_object = create_helm_release(
        test_ns.clone(),
        AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME.to_string(),
        CURRENT_AC_VERSION.to_string(),
        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
    );
    k8s_client
        .apply_dynamic_object(&ac_dynamic_object)
        .expect("no error should occur during the creation of the helm release");

    let cd_dynamic_object = create_helm_release(
        test_ns.clone(),
        AGENT_CONTROL_CD_RELEASE_NAME.to_string(),
        CURRENT_CD_VERSION.to_string(),
        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
    );
    k8s_client
        .apply_dynamic_object(&cd_dynamic_object)
        .expect("no error should occur during the creation of the helm release");

    updater
        .update(config_to_update)
        .expect("no error should occur during update");

    retry(15, Duration::from_secs(5), || {
        verify_helm_release_state(
            &k8s_client,
            &test_ns,
            AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME,
            NEW_AC_VERSION,
            AGENT_CONTROL_VERSION_SET_FROM,
        )?;

        verify_helm_release_state(
            &k8s_client,
            &test_ns,
            AGENT_CONTROL_CD_RELEASE_NAME,
            NEW_CD_VERSION,
            AGENT_CONTROL_VERSION_SET_FROM,
        )?;

        Ok(())
    })
}

fn verify_helm_release_state(
    k8s_client: &Arc<SyncK8sClient>,
    namespace: &str,
    release_name: &str,
    expected_version: &str,
    version_source_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let obj = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name, namespace)?
        .ok_or_else(|| format!("HelmRelease '{release_name}' not found"))?;

    let labels = obj.metadata.labels.as_ref().ok_or("Object has no labels")?;

    if !labels.contains_key(version_source_label) {
        return Err(format!("Object '{release_name}' is missing a required label").into());
    }

    check_version_and_source(
        k8s_client,
        expected_version,
        REMOTE_VAL,
        namespace,
        release_name,
        version_source_label,
    )?;

    Ok(())
}

/// Helper function to create a DynamicObject that simulates a HelmRelease.
fn create_helm_release(
    namespace: String,
    release_name: String,
    version: String,
    main_label: String,
) -> DynamicObject {
    DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(release_name.clone()),
            namespace: Some(namespace),
            labels: Some(BTreeMap::from([(main_label, LOCAL_VAL.to_string())])),
            ..Default::default()
        },
        data: serde_json::json!({
            "spec": {
                "interval": "5m",
                "suspend": true,
                "chart": {
                    "spec": {
                        "chart": "test",
                        "version": version,
                        "sourceRef": {
                            "kind": "HelmRepository",
                            "name": release_name,
                        },
                    },
                }
            }
        }),
    }
}
