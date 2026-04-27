pub mod verify;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{AgentControlDynamicConfig, AgentControlPackage};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::agent_type::runtime_config::on_host::package::OCI_REGISTRY_PLACEHOLDER;
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::oci::reference_parser::ReferenceParser;
use crate::package::manager::{PackageData, PackageManager};
use oci_client::Reference;
use self_replacer::{BinarySelfReplacer, SelfReplacer};
use std::str::FromStr;
use thiserror::Error;
use tracing::{debug, debug_span};
use url::Url;
use verify::VerifyExecutor;

#[cfg(target_family = "unix")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control";
#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control.exe";

pub const AGENT_CONTROL_BIN_PACKAGE_ID: &str = "agent_control_bin";

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid OCI reference in package config: {0}")]
    InvalidReference(#[from] oci_client::ParseError),
}

pub struct OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
    ac_remote_update_enabled: bool,
    agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    package_manager: P,
    verify_executor: V,
    base_reference: Reference,
    pub_key_url: Url,
}

impl<P, V> VersionUpdater for OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        if !self.ac_remote_update_enabled {
            debug!("Remote update is disabled, skipping update process");
            return Ok(());
        }

        let Some(new_version) = &config.version else {
            debug!("Version is not specified in the dynamic config");
            return Ok(());
        };

        let _span = debug_span!(
            "self-update",
            previous_version = AGENT_CONTROL_VERSION,
            new_version = %new_version,
        )
        .entered();

        if new_version == AGENT_CONTROL_VERSION {
            debug!("Desired version is the same as current, skipping update");
            return Ok(());
        }

        debug!("Starting update process");

        let package_data = self.get_package_data(new_version);

        let new_binary_path = self
            .package_manager
            .install(&AgentID::AgentControl, package_data)
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("installing new Agent Control binary: {e}"))
            })?
            .installation_path
            .join(AGENT_CONTROL_BIN);

        debug!(
            binary = %new_binary_path.display(),
            "Verifying new binary before self-replace",
        );
        self.verify_executor
            .execute(&new_binary_path, &["verify"])
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("verifying new Agent Control binary: {e}"))
            })?;

        debug!("Attempting to self-replace with new binary",);

        BinarySelfReplacer::self_replace(&new_binary_path).map_err(|e| {
            UpdaterError::UpdateFailed(format!("self replacing Agent Control binary: {e}"))
        })?;

        debug!("Agent Control binary replaced, stopping to allow the new version to start");
        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::SelfUpdateRestartRequested())
            .map_err(|e| UpdaterError::UpdateFailed(format!("publishing stop request: {e}")))?;

        Ok(())
    }
}

impl<P, V> OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
    pub fn try_new(
        ac_remote_update_enabled: bool,
        agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
        package_manager: P,
        verify_executor: V,
        package: AgentControlPackage,
    ) -> Result<Self, BuildError> {
        let base_reference = Reference::from(ReferenceParser::from_str(
            format!(
                "{}/{}",
                OCI_REGISTRY_PLACEHOLDER, package.download.oci.repository
            )
            .as_str(),
        )?);
        Ok(Self {
            ac_remote_update_enabled,
            agent_control_internal_publisher,
            package_manager,
            verify_executor,
            base_reference,
            pub_key_url: package.download.oci.public_key_url,
        })
    }

    fn get_package_data(&self, new_version: &str) -> PackageData {
        let reference = Reference::with_tag(
            self.base_reference.registry().to_string(),
            self.base_reference.repository().to_string(),
            new_version.to_string(),
        );
        PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci_reference: reference,
            public_key_url: Some(self.pub_key_url.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::{AgentControlPackage, Download, Oci};
    use crate::event::channel::pub_sub;
    use crate::package::manager::tests::MockPackageManager;
    use assert_matches::assert_matches;
    use mockall::mock;
    use std::path::Path;
    use url::Url;

    mock! {
        pub VerifyExecutorMock {}
        impl verify::VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), verify::VerifyError>;
        }
    }

    type TestUpdater = OnHostACUpdater<MockPackageManager, MockVerifyExecutorMock>;

    fn new_test_updater(ac_remote_update_enabled: bool) -> TestUpdater {
        let (publisher, _) = pub_sub();
        OnHostACUpdater::try_new(
            ac_remote_update_enabled,
            publisher,
            MockPackageManager::new(),
            MockVerifyExecutorMock::new(),
            AgentControlPackage::default(),
        )
        .unwrap()
    }

    #[test]
    fn update_is_noop_when_remote_update_disabled() {
        let updater = new_test_updater(false);
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_not_specified() {
        let updater = new_test_updater(true);
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_matches_current() {
        let updater = new_test_updater(true);
        let config = AgentControlDynamicConfig {
            version: Some(AGENT_CONTROL_VERSION.to_string()),
            ..Default::default()
        };
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn try_new_fails_with_invalid_oci_reference() {
        let (publisher, _) = pub_sub();
        let package = AgentControlPackage {
            download: Download {
                oci: Oci {
                    repository: "wrongrepo?".to_string(),
                    public_key_url: Url::parse("https://newrelic.com/keys").unwrap(),
                },
            },
        };
        assert_matches!(
            TestUpdater::try_new(
                true,
                publisher,
                MockPackageManager::new(),
                MockVerifyExecutorMock::new(),
                package
            )
            .err()
            .unwrap(),
            BuildError::InvalidReference(_)
        );
    }
}
