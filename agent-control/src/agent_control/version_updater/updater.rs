use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{AgentControlDynamicConfig, Oci};
use crate::agent_control::defaults::{AGENT_CONTROL_ID, AGENT_CONTROL_VERSION};
use crate::agent_control::version_updater::on_host::VerifyExecutor;
use crate::agent_type::error::AgentTypeError;
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::oci::reference_parser::ReferenceParser;
use crate::package::manager::{PackageData, PackageManager};
use core::str::FromStr;
use oci_client::Reference;
use self_replacer::SelfReplacer;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;
use url::Url;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("update failed: {0}")]
    UpdateFailed(String),
}

/// A trait for updating the agent control version using a dynamic configuration.
///
/// Implementers of this trait are responsible for notifying an external controller
/// about the desired agent control version, as specified in the provided
/// [`AgentControlDynamicConfig`].
pub trait VersionUpdater {
    /// Verifies if the agent control version should be updated based on the provided configuration and
    /// attempts to update the desired agent control version.
    ///
    /// Returns `Ok(())` if the desired version has been successfully communicated
    /// to the external controller, or an `UpdaterError` if the update fails.
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
}

pub struct OnHostACUpdater<S, P, V>
where
    S: SelfReplacer,
    P: PackageManager,
    V: VerifyExecutor,
{
    pub ac_remote_update_enabled: bool,
    pub agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    pub self_replacer: S,
    pub package_manager: Arc<P>,
    pub verify_executor: V,
    pub reference: Oci,
}

impl<S, P, V> VersionUpdater for OnHostACUpdater<S, P, V>
where
    S: SelfReplacer,
    P: PackageManager,
    V: VerifyExecutor,
{
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        if !self.ac_remote_update_enabled {
            debug!("Remote update is disabled, skipping update process");
            return Ok(());
        }
        let Some(new_version) = config.version else {
            debug!("Version is not specified in the dynamic config");
            return Ok(());
        };

        if new_version == AGENT_CONTROL_VERSION {
            debug!(
                "Desired agent control version {new_version} is the same as the current version, skipping update process"
            );
            return Ok(());
        }

        debug!(
            "Starting update process for agent control version {}",
            new_version
        );

        let string_reference = format!(
            "{}/{}{}",
            self.reference.registry, self.reference.repository, new_version
        );

        let reference = Reference::from(
            ReferenceParser::from_str(string_reference.as_str()).map_err(|err| {
                UpdaterError::UpdateFailed(format!("cannot parse reference: {err}"))
            }),
        );

        let public_key_url = self
            .reference
            .public_key_url
            .map(|s| Url::parse(&s))
            .transpose()
            .map_err(|err| UpdaterError::UpdateFailed(format!("invalid public_key_url: {err}")))?;

        let data = PackageData {
            id: "binary".to_string(),
            oci_reference: reference,
            public_key_url,
        };

        let new_binary = self
            .package_manager
            .install(&AgentID::AgentControl, data)
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?
            .installation_path;

        debug!(
            "Verifying new binary {} before self-replace",
            new_binary.to_string_lossy()
        );
        self.verify_executor
            .execute(&new_binary, &vec!["verify"])
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        debug!(
            "Attempting to self-replace with new binary {}",
            new_binary.to_string_lossy()
        );
        self.self_replacer
            .self_replace(&new_binary)
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::StopRequested())
            .unwrap();

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub VersionUpdater {}
        impl VersionUpdater for VersionUpdater {
            fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
        }
    }

    impl MockVersionUpdater {
        /// Returns a mock that always returns `Ok()` regardless of the times it is called
        pub fn new_no_op() -> Self {
            let mut mock = Self::new();
            mock.expect_update().returning(|_| Ok(()));
            mock
        }
    }
}
