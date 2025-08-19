use crate::agent_control::config::{AgentControlDynamicConfig, helmrelease_v2_type_meta};
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::cli::install::agent_control::AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, REMOTE_VAL};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy)]
pub enum Component {
    AgentControl,
    FluxCD,
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Component::AgentControl => write!(f, "Agent Control"),
            Component::FluxCD => write!(f, "Flux"),
        }
    }
}
pub struct K8sACUpdater {
    ac_remote_update: bool,
    cd_remote_update: bool,
    k8s_client: Arc<SyncK8sClient>,
    namespace: String,
    // current_chart_version is the version of the agent control that is currently running.
    // It is loaded at startup, and it is populated by the HelmChart.
    current_chart_version: String,
    // release name for agent control cd loaded from config
    cd_release_name: String,
}

impl VersionUpdater for K8sACUpdater {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        if self.ac_remote_update {
            self.update_helm_release_version(
                Component::AgentControl,
                config.chart_version.as_ref(),
                &self.current_chart_version,
                AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME,
            )?;
        }
        if !self.ac_remote_update {
            debug!("Remote updates for Agent Control are disabled. Nothing to do.");
        }

        if self.cd_remote_update {
            let current_version = self.get_cd_helm_release_version()?;

            self.update_helm_release_version(
                Component::FluxCD,
                config.cd_chart_version.as_ref(),
                current_version.as_str(),
                self.cd_release_name.as_str(),
            )?;
        }
        if !self.cd_remote_update {
            debug!("Remote updates for Agent Control cd are disabled. Nothing to do.");
        }

        Ok(())
    }
}

impl K8sACUpdater {
    pub fn new(
        ac_remote_update: bool,
        cd_remote_update: bool,
        k8s_client: Arc<SyncK8sClient>,
        namespace: String,
        current_chart_version: String,
        cd_deployment_name: String,
    ) -> Self {
        Self {
            ac_remote_update,
            cd_remote_update,
            k8s_client,
            namespace,
            current_chart_version,
            cd_release_name: cd_deployment_name,
        }
    }

    fn create_helm_release_patch(
        &self,
        version: &str,
        mut labels: BTreeMap<String, String>,
    ) -> serde_json::Value {
        labels.insert(
            AGENT_CONTROL_VERSION_SET_FROM.to_string(),
            REMOTE_VAL.to_string(),
        );
        serde_json::json!({
            "metadata":{
                "labels": labels,
            },
            "spec": {
                "chart": {
                    "spec": {
                        "version": version,
                    }
                },
            }
        })
    }

    fn get_helm_release_labels(
        &self,
        release_name: &str,
    ) -> Result<BTreeMap<String, String>, UpdaterError> {
        Ok(self
            .k8s_client
            .get_dynamic_object(&helmrelease_v2_type_meta(), release_name, &self.namespace)
            .map_err(|err| {
                UpdaterError::UpdateFailed(format!(
                    "error fetching {release_name} helmRelease: {err}",
                ))
            })?
            .map(|obj| obj.metadata.clone().labels.unwrap_or_default())
            .unwrap_or_default())
    }

    /// Updates a HelmRelease resource in Kubernetes to a new version.
    ///
    /// If the new version is specified and differs from the current version, this function
    /// applies a JSON patch to the HelmRelease to update its `spec.chart.spec.version`.
    fn update_helm_release_version(
        &self,
        component_name: Component,
        new_version: Option<&String>,
        current_version: &str,
        release_name: &str,
    ) -> Result<(), UpdaterError> {
        let Some(version) = new_version else {
            debug!("Version for '{component_name}' is not specified in the dynamic config.");
            return Ok(());
        };

        if version == current_version {
            debug!("Current version of '{component_name}' is already up to date: {version}");
            return Ok(());
        }

        info!("Updating '{component_name}' from version: {current_version} to {version}");

        let labels = self.get_helm_release_labels(release_name)?;
        let patch_to_apply = self.create_helm_release_patch(version, labels);

        self.k8s_client
            .patch_dynamic_object(
                &helmrelease_v2_type_meta(),
                release_name,
                &self.namespace,
                patch_to_apply,
            )
            .map_err(|err| {
                UpdaterError::UpdateFailed(format!(
                    "Error applying patch to HelmRelease '{release_name}' for '{component_name}': {err}",
                ))
            })?;
        Ok(())
    }

    fn get_cd_helm_release_version(&self) -> Result<String, UpdaterError> {
        let helm_release = self
            .k8s_client
            .get_dynamic_object(
                &helmrelease_v2_type_meta(),
                &self.cd_release_name,
                &self.namespace,
            )
            .map_err(|k8s_err| {
                UpdaterError::UpdateFailed(format!(
                    "Failed to fetch HelmRelease '{}': {}",
                    &self.cd_release_name, k8s_err
                ))
            })?
            .ok_or_else(|| {
                UpdaterError::UpdateFailed(format!(
                    "HelmRelease '{}' not found",
                    &self.cd_release_name
                ))
            })?;

        let version = helm_release
            .data
            .get("spec")
            .and_then(|spec| spec.get("chart"))
            .and_then(|chart| chart.get("spec"))
            .and_then(|chart_spec| chart_spec.get("version"))
            .and_then(|version_val| version_val.as_str())
            .map(String::from)
            .ok_or_else(|| {
                UpdaterError::UpdateFailed(format!(
                    "Could not find version at 'spec.chart.spec.version' in HelmRelease '{}'",
                    &self.cd_release_name
                ))
            })?;

        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::install::agent_control::AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME;
    use crate::k8s::Error as K8sError;
    use crate::k8s::client::MockSyncK8sClient;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::DynamicObject;
    use mockall::predicate::*;
    use serde_json::json;

    const TEST_NAMESPACE: &str = "test-ns";
    const CURRENT_AC_VERSION: &str = "1.0.0";
    const NEW_AC_VERSION: &str = "1.1.0";
    const CURRENT_CD_VERSION: &str = "2.0.0";
    const NEW_CD_VERSION: &str = "2.1.0";
    const CD_RELEASE_NAME_TEST: &str = "flux-cd";

    /// Creates a dynamic configuration for tests.
    fn test_config(
        ac_version: Option<&str>,
        cd_version: Option<&str>,
    ) -> AgentControlDynamicConfig {
        AgentControlDynamicConfig {
            chart_version: ac_version.map(String::from),
            cd_chart_version: cd_version.map(String::from),
            ..Default::default()
        }
    }

    /// Creates a DynamicObject mock to simulate a HelmRelease.
    fn mock_helm_release(
        name: &str,
        version: &str,
        labels: BTreeMap<String, String>,
    ) -> DynamicObject {
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            data: json!({
                "spec": { "chart": { "spec": { "version": version } } }
            }),
        }
    }

    #[test]
    fn test_update_only_agent_control_when_enabled() {
        let mut mock_client = MockSyncK8sClient::new();

        // Expect AC labels to be fetched
        mock_client
            .expect_get_dynamic_object()
            .with(
                eq(helmrelease_v2_type_meta()),
                eq(AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME),
                eq(TEST_NAMESPACE),
            )
            .times(1)
            .returning(|_, _, _| {
                Ok(Some(Arc::new(mock_helm_release(
                    AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME,
                    CURRENT_AC_VERSION,
                    BTreeMap::new(),
                ))))
            });

        // Expect only AC to be patched
        mock_client
            .expect_patch_dynamic_object()
            .withf(|_, name, _, patch| {
                name == AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME
                    && patch
                        .pointer("/spec/chart/spec/version")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        == NEW_AC_VERSION
            })
            .times(1)
            .returning(|_, _, _, _| {
                Ok(mock_helm_release(
                    AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME,
                    CURRENT_AC_VERSION,
                    BTreeMap::new(),
                ))
            });

        let updater = K8sACUpdater::new(
            true,
            false,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        let result = updater.update(&test_config(Some(NEW_AC_VERSION), None));

        assert!(result.is_ok());
    }

    #[test]
    fn test_update_only_flux_when_enabled() {
        let mut mock_client = MockSyncK8sClient::new();

        // Expect CD version and labels to be fetched
        mock_client
            .expect_get_dynamic_object()
            .with(
                eq(helmrelease_v2_type_meta()),
                eq(CD_RELEASE_NAME_TEST),
                eq(TEST_NAMESPACE),
            )
            .times(2)
            .returning(|_, _, _| {
                Ok(Some(Arc::new(mock_helm_release(
                    CD_RELEASE_NAME_TEST,
                    CURRENT_CD_VERSION,
                    BTreeMap::new(),
                ))))
            });

        // Expect only Flux/CD to be patched
        mock_client
            .expect_patch_dynamic_object()
            .withf(|_, name, _, patch| {
                name == CD_RELEASE_NAME_TEST
                    && patch
                        .pointer("/spec/chart/spec/version")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        == NEW_CD_VERSION
            })
            .times(1)
            .returning(|_, _, _, _| {
                Ok(mock_helm_release(
                    CD_RELEASE_NAME_TEST,
                    CURRENT_CD_VERSION,
                    BTreeMap::new(),
                ))
            });

        let updater = K8sACUpdater::new(
            false,
            true,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        let result = updater.update(&test_config(None, Some(NEW_CD_VERSION)));

        assert!(result.is_ok());
    }
    #[test]
    fn test_update_both_when_both_enabled() {
        let mut mock_client = MockSyncK8sClient::new();

        // Expect calls for both: AC and CD
        mock_client
            .expect_get_dynamic_object()
            .returning(|_, name, _| {
                if name == AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME {
                    Ok(Some(Arc::new(mock_helm_release(
                        AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME,
                        CURRENT_AC_VERSION,
                        BTreeMap::new(),
                    ))))
                } else if name == CD_RELEASE_NAME_TEST {
                    Ok(Some(Arc::new(mock_helm_release(
                        CD_RELEASE_NAME_TEST,
                        CURRENT_CD_VERSION,
                        BTreeMap::new(),
                    ))))
                } else {
                    Ok(None)
                }
            });

        // Expect two patch calls
        mock_client
            .expect_patch_dynamic_object()
            .times(2)
            .returning(|_, name, _, patch| {
                let version = patch
                    .pointer("/spec/chart/spec/version")
                    .unwrap()
                    .as_str()
                    .unwrap();
                Ok(mock_helm_release(name, version, BTreeMap::new()))
            });

        let updater = K8sACUpdater::new(
            true,
            true,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        let result = updater.update(&test_config(Some(NEW_AC_VERSION), Some(NEW_CD_VERSION)));

        assert!(result.is_ok());
    }

    #[test]
    fn test_does_nothing_when_both_disabled() {
        let mock_client = MockSyncK8sClient::new();
        let updater = K8sACUpdater::new(
            false,
            false,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        let result = updater.update(&test_config(Some(NEW_AC_VERSION), Some(NEW_CD_VERSION)));

        assert!(result.is_ok());
    }

    #[test]
    fn test_does_not_patch_if_version_is_the_same() {
        let mut mock_client = MockSyncK8sClient::new();
        // Expect the call to get the CD version, but not to patch
        mock_client
            .expect_get_dynamic_object()
            .with(
                eq(helmrelease_v2_type_meta()),
                eq(CD_RELEASE_NAME_TEST),
                eq(TEST_NAMESPACE),
            )
            .returning(|_, _, _| {
                Ok(Some(Arc::new(mock_helm_release(
                    CD_RELEASE_NAME_TEST,
                    CURRENT_CD_VERSION,
                    BTreeMap::new(),
                ))))
            });

        let updater = K8sACUpdater::new(
            true,
            true,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        // We pass the current versions, not the new ones
        let result = updater.update(&test_config(
            Some(CURRENT_AC_VERSION),
            Some(CURRENT_CD_VERSION),
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn test_returns_error_if_get_helm_release_fails() {
        let mut mock_client = MockSyncK8sClient::new();
        mock_client
            .expect_get_dynamic_object()
            .returning(|_, _, _| Err(K8sError::GetDynamic("API server is down".to_string())));

        let updater = K8sACUpdater::new(
            true,
            false,
            Arc::new(mock_client),
            TEST_NAMESPACE.to_string(),
            CURRENT_AC_VERSION.to_string(),
            CD_RELEASE_NAME_TEST.to_string(),
        );

        let result = updater.update(&test_config(Some(NEW_AC_VERSION), None));

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, UpdaterError::UpdateFailed(_)));
        assert!(error.to_string().contains("API server is down"));
    }
}
