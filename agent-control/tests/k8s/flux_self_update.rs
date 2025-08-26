/// Integration tests for Flux self-update functionality in Kubernetes environments.
///
/// Each test installs its own Flux instance with namespace-scoped RBAC to avoid
/// conflicts with other Flux installations in the same cluster. This requires
/// special setup that differs from other integration tests and is managed by
/// the Makefile and Tiltfile for CI environments.
use crate::common::opamp::FakeServer;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::{DeferredCommand, retry};
use crate::common::runtime::{self, block_on};
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_SPLIT_NS_PATH, start_agent_control_with_testdata_config,
};
use crate::k8s::tools::cmd::print_cli_output;
use crate::k8s::tools::instance_id::get_instance_id;
use crate::k8s::tools::k8s_api::{check_helmrelease_chart_version, create_values_secret};
use crate::k8s::tools::k8s_env::K8sEnv;
use crate::k8s::tools::local_chart::LOCAL_CHART_REPOSITORY;
use crate::k8s::tools::local_chart::agent_control_cd::{
    CHART_VERSION_UPSTREAM_1, CHART_VERSION_UPSTREAM_1_PKG, CHART_VERSION_UPSTREAM_2,
};
use k8s_openapi::api::rbac::v1::{Role, RoleBinding};
use kube::api::PostParams;
use kube::{Api, Client};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::health_checker::k8s::agent_control_health_checker_builder;
use newrelic_agent_control::cli::install::flux::HELM_REPOSITORY_NAME;
use newrelic_agent_control::health::health_checker::{Health, HealthChecker};
use newrelic_agent_control::health::with_start_time::StartTime;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

const TEST_RELEASE_NAME: &str = "test-agent-control-cd";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_and_update_flux_resources_success() {
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // install flux chart (simulate what the install flux job does)
    flux_bootstrap_via_helm_command(k8s.client.clone(), &namespace);

    let ns = namespace.to_string();
    // Flux resources need to be removed before the test ends, otherwise the namespace will fail to be removed
    // as these resources include finalizers pointing to flux.
    let _remove_resources = DeferredCommand::new(move || {
        remove_flux_resources(&ns);
    });

    // Installs flux resources
    create_flux_resources(&namespace, CHART_VERSION_UPSTREAM_1);

    // Upgrade chart version from local
    create_flux_resources(&namespace, CHART_VERSION_UPSTREAM_2);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_remote_flux_update() {
    let test_name = "k8s_remote_flux_update";
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // install flux chart (simulate what the install flux job does)
    flux_bootstrap_via_helm_command(k8s.client.clone(), &namespace);

    let ns = namespace.to_string();
    // Flux resources need to be removed before the test ends, otherwise the namespace will fail to be removed
    // as these resources include finalizers pointing to flux.
    let _remove_resources = DeferredCommand::new(move || {
        remove_flux_resources(&ns);
    });

    // Installs flux resources
    create_flux_resources(&namespace, CHART_VERSION_UPSTREAM_1);

    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &namespace,
        &namespace,
        Some(opamp_server.cert_file_path()),
        Some(&opamp_server.endpoint()),
        vec![],
        tmp_dir.path(),
    );

    let ac_instance_id = get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents: {{}}
cd_chart_version: {CHART_VERSION_UPSTREAM_2}
"#
        ),
    );

    // AC internal health-checker will be unhealthy because the test doesn't install the AC HelmRelease
    // So to verify that Flux is health after the upgrade we create this health-checker
    let health_checker = agent_control_health_checker_builder(
        Arc::new(SyncK8sClient::try_new(runtime::tokio_runtime()).unwrap()),
        namespace.clone(),
        None,
        TEST_RELEASE_NAME.to_string().into(),
    )(StartTime::now())
    .unwrap();

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32, // The configuration was Applied even if it led to an unhealthy AC.
        )?;
        block_on(check_helmrelease_chart_version(
            k8s.client.clone(),
            &namespace,
            TEST_RELEASE_NAME,
            CHART_VERSION_UPSTREAM_2,
        ))?;
        let health = health_checker.check_health()?;
        if let Some(error) = health.last_error() {
            Err(format!("HelmRelease unhealthy with: {error}").into())
        } else {
            Ok(())
        }
    });

    // run a local version updated and asserts that the version doesn't change
    create_flux_resources(&namespace, CHART_VERSION_UPSTREAM_1);

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_chart_version(
            k8s.client.clone(),
            &namespace,
            TEST_RELEASE_NAME,
            CHART_VERSION_UPSTREAM_2,
        ))?;
        let health = health_checker.check_health()?;
        if let Some(error) = health.last_error() {
            Err(format!("HelmRelease unhealthy with: {error}").into())
        } else {
            Ok(())
        }
    });
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_remote_flux_update_with_wrong_version_causes_unhealthy() {
    let test_name = "k8s_remote_flux_update";
    let mut opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // install flux chart (simulate what the install flux job does)
    flux_bootstrap_via_helm_command(k8s.client.clone(), &namespace);

    let ns = namespace.to_string();
    // Flux resources need to be removed before the test ends, otherwise the namespace will fail to be removed
    // as these resources include finalizers pointing to flux.
    let _remove_resources = DeferredCommand::new(move || {
        remove_flux_resources(&ns);
    });

    // Installs flux resources
    create_flux_resources(&namespace, CHART_VERSION_UPSTREAM_1);

    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &namespace,
        &namespace,
        Some(opamp_server.cert_file_path()),
        Some(&opamp_server.endpoint()),
        vec![],
        tmp_dir.path(),
    );

    let ac_instance_id = get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    let unsupported_version = "9999.99.99";
    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents: {{}}
cd_chart_version: {unsupported_version}
"#
        ),
    );

    // AC internal health-checker will be unhealthy because the test doesn't install the AC HelmRelease
    // So to verify that Flux is healthy after the upgrade we create this health-checker
    // for the CD component only using the same constructs as AC does.
    let health_checker = agent_control_health_checker_builder(
        Arc::new(SyncK8sClient::try_new(runtime::tokio_runtime()).unwrap()),
        namespace.clone(),
        None,
        TEST_RELEASE_NAME.to_string().into(),
    )(StartTime::now())
    .unwrap();

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32, // The configuration was Applied even if it led to an unhealthy AC.
        )?;

        block_on(check_helmrelease_chart_version(
            k8s.client.clone(),
            &namespace,
            TEST_RELEASE_NAME,
            unsupported_version,
        ))?;

        let health = health_checker.check_health()?;
        // Should be unhealthy with a specific message
        let expected_err_msg = format!(
            "no 'agent-control-cd' chart with version matching '{unsupported_version}' found"
        );
        match health.as_health() {
            Health::Healthy(_) => Err("HelmRelease should be unhealthy".into()),
            Health::Unhealthy(u) if u.last_error().contains(&expected_err_msg) => Ok(()),
            Health::Unhealthy(u) => {
                Err(format!("Unhealthy for the wrong reason: {}", u.last_error()).into())
            }
        }
    });
}

// HELPERS

const SECRET_NAME: &str = "flux-values";
const VALUES_KEY: &str = "values.yaml";

fn create_flux_resources(namespace: &str, chart_version: &str) {
    let mut cmd = assert_cmd::Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.timeout(Duration::from_secs(60));
    cmd.arg("create-cd-resources");
    cmd.arg("--installation-check-initial-delay").arg("1s");
    cmd.arg("--installation-check-timeout").arg("30s");
    cmd.arg("--log-level").arg("debug");
    cmd.arg("--repository-url").arg(LOCAL_CHART_REPOSITORY);
    cmd.arg("--chart-version").arg(chart_version);
    cmd.arg("--chart-name").arg(HELM_REPOSITORY_NAME);
    cmd.arg("--release-name").arg(TEST_RELEASE_NAME);
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--secrets")
        .arg(format!("{SECRET_NAME}={VALUES_KEY}"));
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();
}

fn remove_flux_resources(namespace: &str) {
    let mut cmd = assert_cmd::Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.timeout(Duration::from_secs(60));
    cmd.arg("remove-cd-resources");
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--release-name").arg(TEST_RELEASE_NAME);
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();
}

/// Bootstraps a Flux installation using the local Helm chart with namespace-scoped RBAC.
///
/// This function replicates the flux install job from the agent-control chart but with
/// key modifications for testing:
/// - Uses the local chart instead of a remote repository
/// - Creates namespace-scoped RBAC instead of cluster-wide permissions
/// - Allows multiple Flux instances in the same cluster without conflicts (flux chart has
///   fixed name cluster wider resources)
///
/// Note: The namespace-scoped RBAC limits the operations this Flux instance can perform
/// compared to a standard cluster-wide installation.
fn flux_bootstrap_via_helm_command(k8s_client: Client, namespace: &str) {
    block_on(create_flux_rbac(k8s_client.clone(), namespace));
    let mut cmd = assert_cmd::Command::new("helm");
    cmd.timeout(Duration::from_secs(90)); // to fail fast in case unrecoverable error.
    cmd.arg("install")
        .arg(TEST_RELEASE_NAME)
        .arg(CHART_VERSION_UPSTREAM_1_PKG)
        .arg("--wait")
        .arg("--namespace")
        .arg(namespace)
        .arg("--dependency-update")
        .arg("--set=flux2.installCRDs=false")
        .arg("--set=flux2.rbac.create=false")
        .arg("--set=flux2.rbac.createAggregation=false");
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    // Create a values secret with the Flux values to be used by the HelmRelease
    create_values_secret(
        k8s_client,
        namespace,
        SECRET_NAME,
        VALUES_KEY,
        r#"
flux2:
  installCRDs: false
  rbac:
    create: false
    createAggregation: false
"#
        .to_string(),
    );
}

/// Creates RBAC resources for Flux in a test namespace.
/// This is needed because Flux is hardcoding names for cluster-wide resources
///
/// This allows multiple Flux installations in the same cluster for testing purposes
/// by creating namespace-scoped ServiceAccounts, Role, and RoleBinding resources.
///
/// When installing Flux with Helm, use these flags to prevent conflicts:
/// ```
/// --set=flux2.installCRDs=false
/// --set=flux2.rbac.create=false
/// --set=flux2.rbac.createAggregation=false
/// ```
async fn create_flux_rbac(k8s_client: Client, namespace: &str) {
    let role_api: Api<Role> = Api::namespaced(k8s_client.clone(), namespace);
    role_api
        .create(
            &PostParams::default(),
            &serde_yaml::from_str(ROLE_FLUX).unwrap(),
        )
        .await
        .unwrap();

    let role_binding_api: Api<RoleBinding> = Api::namespaced(k8s_client.clone(), namespace);
    role_binding_api
        .create(
            &PostParams::default(),
            &serde_yaml::from_str(ROLE_BINDING_FLUX).unwrap(),
        )
        .await
        .unwrap();
}

const ROLE_FLUX: &str = r#"
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: flux
rules:
- apiGroups: ["*"]
  resources: ["*"]
  verbs: ["*"]
"#;
const ROLE_BINDING_FLUX: &str = r#"
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: flux
subjects:
- kind: ServiceAccount
  name: source-controller
- kind: ServiceAccount
  name: helm-controller
roleRef:
  kind: Role
  name: flux
  apiGroup: rbac.authorization.k8s.io
"#;
