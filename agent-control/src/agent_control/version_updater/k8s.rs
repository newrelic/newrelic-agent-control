use crate::agent_control::config::{AgentControlDynamicConfig, helmrelease_v2_type_meta};
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::cli::install_agent_control::RELEASE_NAME;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, REMOTE_VAL};
use kube::api::DynamicObject;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, info};

pub struct K8sACUpdater {
    k8s_client: Arc<SyncK8sClient>,
    // current_chart_version is the version of the agent control that is currently running.
    // It is loaded at startup, and it is populated by the HelmChart.
    current_chart_version: String,
}

impl VersionUpdater for K8sACUpdater {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        let Some(version) = config.chart_version.as_ref() else {
            debug!("Chart version is not specified");
            return Ok(());
        };

        if version == &self.current_chart_version {
            debug!("Current version is already up to date");
            return Ok(());
        }

        // To avoid overwriting existing labels we need to get the object and to add manually the label
        // since the strategic merge is not available for CRs
        let labels = self.get_helmrelease_labels()?;

        let patch_to_apply = self.create_helm_release_patch(version, labels);

        info!(
            "Updating Agent Control to version: {} -> {}",
            self.current_chart_version, version
        );
        self.k8s_client
            .patch_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME, patch_to_apply)
            .map_err(|err| {
                UpdaterError::UpdateFailed(format!(
                    "applying patch to {RELEASE_NAME} helmRelease: {err}",
                ))
            })?;

        Ok(())
    }
}

impl K8sACUpdater {
    pub fn new(k8s_client: Arc<SyncK8sClient>, current_chart_version: String) -> Self {
        Self {
            k8s_client,
            current_chart_version,
        }
    }

    fn create_helm_release_patch(
        &self,
        version: &String,
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

    fn get_helmrelease_labels(&self) -> Result<BTreeMap<String, String>, UpdaterError> {
        Ok(self
            .k8s_client
            .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
            .map_err(|err| {
                UpdaterError::UpdateFailed(format!(
                    "error fetching {RELEASE_NAME} helmRelease: {err}",
                ))
            })?
            .map(|obj| obj.metadata.clone().labels.unwrap_or_default())
            .unwrap_or_default())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_missing_chart_version_does_no_op() {
        let mut k8s_client = SyncK8sClient::default();
        k8s_client.expect_patch_dynamic_object().never();

        let updater = K8sACUpdater::new(Arc::new(k8s_client), "1.0.0".to_string());

        updater
            .update(&AgentControlDynamicConfig {
                chart_version: None,
                ..Default::default()
            })
            .expect("updater should return Ok without making any calls to the k8s client");
    }
    #[test]
    fn test_update_to_current_version_does_no_op() {
        let mut k8s_client = SyncK8sClient::default();
        k8s_client.expect_patch_dynamic_object().never();

        let current_version = "1.0.0".to_string();

        let updater = K8sACUpdater::new(Arc::new(k8s_client), current_version.clone());

        updater
            .update(&AgentControlDynamicConfig {
                chart_version: Some(current_version),
                ..Default::default()
            })
            .expect("updater should return Ok without making any calls to the k8s client");
    }

    // Update test case is covered with an integration test.
}
