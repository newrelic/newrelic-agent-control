use error::ConfigStatusManagerError;
use opamp_client::operation::capabilities::Capabilities;
use tracing::debug;

use crate::{agent_control::config::AgentID, values::yaml_config::YAMLConfig};

use super::status::AgentRemoteConfigStatus;

pub mod error;
#[cfg(feature = "k8s")]
pub mod k8s;
pub mod local_filesystem;

/// This trait represents the ability to persist and retrieve the
/// configuration status for an agent at a given point in time.
///
/// The local configuration status retrieved will be the actual values of the configuration,
/// while the remote status will be both the values and the hash provided by the OpAMP server,
/// in the form of a [AgentRemoteConfigStatus] value.
///
/// An implementer of this trait will have the capability to retrieve the local config
/// and retrieve, store and delete the remote configuration status.
pub trait ConfigStatusManager {
    /// Looks for remote configs first, if unavailable checks the local ones.
    /// If none is found, it fallbacks to the empty default values.
    fn load_remote_fallback_local(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<YAMLConfig, ConfigStatusManagerError> {
        debug!(agent_id = agent_id.to_string(), "loading config");

        if let Some(values_result) = self
            .retrieve_remote_status(agent_id, capabilities)?
            // If hash is failed we do not use this remote config
            .filter(|status| !status.status_hash.is_failed())
            .and_then(|status| status.remote_config)
        {
            return Ok(values_result);
        }
        debug!(
            agent_id = agent_id.to_string(),
            "remote config not found, loading local"
        );

        if let Some(values_result) = self.retrieve_local_config(agent_id)? {
            return Ok(values_result);
        }
        debug!(
            agent_id = agent_id.to_string(),
            "local config not found, falling back to defaults"
        );
        Ok(YAMLConfig::default())
    }

    /// Retrieve the current local configuration status for the given agent (if any).
    fn retrieve_local_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, ConfigStatusManagerError>;

    /// Retrieve the current remote configuration status for the given agent (if any).
    fn retrieve_remote_status(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<AgentRemoteConfigStatus>, ConfigStatusManagerError>;

    /// Store the current remote configuration status for the given agent.
    fn store_remote_status(
        &self,
        agent_id: &AgentID,
        status: &AgentRemoteConfigStatus,
    ) -> Result<(), ConfigStatusManagerError>;

    /// Delete the current remote configuration status for the given agent.
    fn delete_remote_status(&self, agent_id: &AgentID) -> Result<(), ConfigStatusManagerError>;
}

#[cfg(test)]
pub mod tests {
    use mockall::mock;

    use super::*;

    mock! {
      pub ConfigStatusManagerMock {}

      impl ConfigStatusManager for ConfigStatusManagerMock {

          fn retrieve_local_config(&self, agent_id: &AgentID) -> Result<Option<YAMLConfig>, ConfigStatusManagerError>;
          fn retrieve_remote_status(
              &self,
              agent_id: &AgentID,
              capabilities: &Capabilities,
          ) -> Result<Option<AgentRemoteConfigStatus>, ConfigStatusManagerError>;
          fn store_remote_status(
              &self,
              agent_id: &AgentID,
              status: &AgentRemoteConfigStatus,
          ) -> Result<(), ConfigStatusManagerError>;
          fn delete_remote_status(&self, agent_id: &AgentID) -> Result<(), ConfigStatusManagerError>;
      }
    }
}
