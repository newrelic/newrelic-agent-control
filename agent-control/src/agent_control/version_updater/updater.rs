use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{AgentControlDynamicConfig, Package};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::on_host::VerifyExecutor;
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

pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control";

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
    pub reference: Option<Package>,
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

        let Some(new_version) = &config.version else {
            debug!("Version is not specified in the dynamic config");
            return Ok(());
        };

        if new_version == AGENT_CONTROL_VERSION {
            debug!(
                "Desired agent control version {new_version} is the same as the current version, skipping update process"
            );
            return Ok(());
        }

        debug!("Starting update process for agent control version {new_version}");

        let Some(package) = &self.reference else {
            return Err(UpdaterError::UpdateFailed(
                "package reference is not specified in the updater, cannot proceed with the update process".to_string(),
            ));
        };

        let package_data = Self::get_package_data(new_version, package)?;

        let new_binary_path = self
            .package_manager
            .install(&AgentID::AgentControl, package_data)
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?
            .installation_path
            .join(AGENT_CONTROL_BIN);

        debug!(
            "Verifying new binary {} before self-replace",
            new_binary_path.to_string_lossy()
        );
        self.verify_executor
            .execute(&new_binary_path, &vec!["verify"])
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        debug!(
            "Attempting to self-replace with new binary {}",
            new_binary_path.to_string_lossy()
        );

        //TODO we should consider managing the errors that can happen in the self-replace process
        S::self_replace(&new_binary_path).map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        debug!(
            "Successfully updated agent control to version, stopping the agent control to allow the new version to start",
        );
        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::StopRequested())
            .unwrap();

        Ok(())
    }
}

impl<S, P, V> OnHostACUpdater<S, P, V>
where
    P: PackageManager,
    S: SelfReplacer,
    V: VerifyExecutor,
{
    fn get_package_data(
        new_version: &String,
        package: &Package,
    ) -> Result<PackageData, UpdaterError> {
        let public_key_url = package
            .download
            .oci
            .public_key_url
            .clone()
            .map(|s| Url::parse(&s))
            .transpose()
            .map_err(|err| UpdaterError::UpdateFailed(format!("invalid public_key_url: {err}")))?;

        let string_reference = format!(
            "{}/{}{}",
            package.download.oci.registry, package.download.oci.repository, new_version
        );

        let reference = Reference::from(
            ReferenceParser::from_str(string_reference.as_str()).map_err(|err| {
                UpdaterError::UpdateFailed(format!("cannot parse reference: {err}"))
            })?,
        );

        let package_data = PackageData {
            id: "binary".to_string(),
            oci_reference: reference,
            public_key_url,
        };
        Ok(package_data)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::config::Oci;
    use crate::agent_control::version_updater::on_host::ProcessVerifyExecutor;
    use crate::package::manager::tests::MockPackageManager;
    use mockall::mock;
    use rstest::rstest;

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

    /// Mock SelfReplacer for testing - never actually called by get_package_data
    struct MockSelfReplacer;
    impl SelfReplacer for MockSelfReplacer {
        type Error = std::io::Error;

        fn self_replace(_new_bin: impl AsRef<std::path::Path>) -> Result<(), Self::Error> {
            panic!("MockSelfReplacer::self_replace should never be called in these tests")
        }
    }

    type TestUpdater = OnHostACUpdater<MockSelfReplacer, MockPackageManager, ProcessVerifyExecutor>;

    #[rstest]
    #[case("registry.io", "repo/binary:", "v1.0.0", None)]
    #[case(
        "ghcr.io",
        "org/pkg:",
        "v1.2.3",
        Some("https://keys.example.com/jwks.json")
    )]
    fn test_get_package_data_parses_reference(
        #[case] registry: &str,
        #[case] repository: &str,
        #[case] new_version: &str,
        #[case] public_key_url: Option<&str>,
    ) {
        let package = Package {
            download: crate::agent_control::config::Download {
                oci: Oci {
                    registry: registry.to_string(),
                    repository: repository.to_string(),
                    version: String::new(),
                    public_key_url: public_key_url.map(|s| s.to_string()),
                },
            },
        };

        let result = TestUpdater::get_package_data(&new_version.to_string(), &package);

        let data = result.unwrap();
        assert_eq!(data.id, "binary");
        assert!(data.oci_reference.to_string().contains(registry));
        assert_eq!(data.public_key_url.is_some(), public_key_url.is_some());
    }

    #[rstest]
    fn test_get_package_data_fails_with_invalid_url() {
        let package = Package {
            download: crate::agent_control::config::Download {
                oci: Oci {
                    registry: "registry.io".to_string(),
                    repository: "repo:".to_string(),
                    version: String::new(),
                    public_key_url: Some("not a valid url".to_string()),
                },
            },
        };

        let result = TestUpdater::get_package_data(&"v1.0.0".to_string(), &package);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid public_key_url")
        );
    }
}
