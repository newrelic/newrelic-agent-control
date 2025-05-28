use super::config::AgentControlDynamicConfig;
use crate::agent_control::config::helmrelease_v2_type_meta;
use crate::k8s::client::SyncK8sClient;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("update failed: {0}")]
    UpdateFailed(String),
}

pub trait Updater {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
}

pub struct NoOpUpdater;

impl Updater for NoOpUpdater {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        Ok(())
    }
}

pub struct K8sUpdater {
    k8s_client: Arc<SyncK8sClient>,
}

impl K8sUpdater {
    pub fn new(k8s_client: Arc<SyncK8sClient>) -> Self {
        Self { k8s_client }
    }
}

impl Updater for K8sUpdater {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        // TODO add this to the AC k8s config?
        // Is set as the release name in the chart.
        let helm_release_name = "agent-control";

        // TODO this is coupled with the helmrelease version so it should somehow be connected with helmrelease_v2_type_meta()
        // and helmrepository_type_meta().
        // It might ended up as a todo because the same thing happens in other parts of the codebase like in the healthcheckers.
        let patch = serde_json::json!({
            "spec": {
                "chart": {
                  "spec": {
                    "version": config.chart_version,
                }
              }
            }
        });

        self.k8s_client
            .patch_dynamic_object(&helmrelease_v2_type_meta(), helm_release_name, patch)
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("Failed to update HelmRelease: {}", e))
            })?;

        Ok(())
    }
}
