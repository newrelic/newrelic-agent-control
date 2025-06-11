use crate::agent_control::config::{AgentControlDynamicConfig, helmrelease_v2_type_meta};
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::cli::install_agent_control::RELEASE_NAME;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
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
            return Err(UpdaterError::UpdateFailed(
                "chart version is not specified".to_string(),
            ));
        };

        let patch_to_apply = self.create_helm_release_patch(version);

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

    fn should_update(&self, config: &AgentControlDynamicConfig) -> bool {
        let Some(version) = &config.chart_version else {
            debug!(
                "AgentControl will not be updated since chart version is not specified in the config"
            );
            return false;
        };

        debug!(
            "Checking if update is needed: current version {}, target version {}",
            self.current_chart_version, version
        );
        &self.current_chart_version != version
    }
}

impl K8sACUpdater {
    pub fn new(k8s_client: Arc<SyncK8sClient>, current_chart_version: String) -> Self {
        Self {
            k8s_client,
            current_chart_version,
        }
    }

    fn create_helm_release_patch(&self, version: &String) -> serde_json::Value {
        serde_json::json!({
            "spec": {
                "chart": {
                    "spec": {
                        "version": version,
                    }
                },
            }
        })
    }
}
