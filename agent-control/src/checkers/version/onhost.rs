use std::process::Command;

use crate::agent_control::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
use crate::agent_type::runtime_config::on_host::executable::rendered::Args;
use crate::agent_type::runtime_config::on_host::rendered::RenderedPackages;
use crate::opamp::attributes::{UpdatedAttributesMessage, publish_update_attributes_event};
use opamp_client::operation::settings::AgentDescription;
use regex::Regex;
use std::collections::HashMap;

use crate::checkers::version::{AgentVersion, VersionCheckError, VersionChecker};
use crate::event::channel::EventPublisher;
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use std::fmt::Debug;
use tracing::{debug, info, info_span, warn};

pub struct OnHostAgentVersionChecker {
    pub(crate) path: Option<String>,
    pub(crate) args: Option<Args>,
    pub(crate) regex: Option<Regex>,
    pub(crate) packages: Option<RenderedPackages>,
}

impl VersionChecker for OnHostAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        // Priority 1: Try OCI package version
        if let Some(version) = self.extract_package_version() {
            debug!(version = %version, "Using version from OCI package metadata");
            return Ok(AgentVersion {
                version,
                opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
            });
        }

        // Priority 2: Fall back to command-based checking
        if let (Some(path), Some(args)) = (&self.path, &self.args) {
            debug!(path = %path, "Using command-based version checking as fallback");
            let output = Command::new(path)
                .args(args.0.clone())
                .output()
                .map_err(|e| {
                    warn!("Command-based version check failed: {e}");
                    VersionCheckError(format!("error executing version command: {e}"))
                })?;
            let output = String::from_utf8_lossy(&output.stdout);

            let version = if let Some(regex) = &self.regex {
                let version_match = regex.find(&output).ok_or(VersionCheckError(
                    "error checking agent version: version not found in command output".to_string(),
                ))?;
                version_match.as_str().to_string()
            } else {
                output.trim().to_string()
            };

            return Ok(AgentVersion {
                version,
                opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
            });
        }

        // No version source available
        Err(VersionCheckError(
            "no version source available (no packages, no command config)".to_string(),
        ))
    }
}

impl OnHostAgentVersionChecker {
    /// Extracts version from OCI package metadata.
    ///
    /// Priority:
    /// 1. Tag (e.g., "1.2.3", "v1.2.3")
    /// 2. Digest (truncated to first 12 chars)
    ///
    /// Version normalization:
    /// - Strips 'v' prefix if present (v1.2.3 → 1.2.3)
    fn extract_package_version(&self) -> Option<String> {
        let packages = self.packages.as_ref()?;

        if packages.is_empty() {
            return None;
        }

        // Use first package (most agents have single package)
        let package = packages.values().next()?;
        let version = &package.download.oci.version;

        // Try tag first (preferred)
        let (tag, digest) = version.tag_and_digest();

        if let Some(tag) = tag {
            let normalized = Self::normalize_version(&tag);
            return Some(normalized);
        }

        // Fall back to digest (truncated for readability)
        if let Some(digest) = digest {
            // Extract short hash (first 12 chars after 'sha256:')
            let short_digest = digest
                .strip_prefix("sha256:")
                .and_then(|d| d.get(..12))
                .unwrap_or(&digest);
            return Some(format!("digest-{}", short_digest));
        }

        None
    }

    /// Normalizes version string by stripping 'v' prefix.
    ///
    /// Examples:
    /// - "v1.2.3" → "1.2.3"
    /// - "1.2.3" → "1.2.3"
    /// - "v7" → "7"
    fn normalize_version(version: &str) -> String {
        version.strip_prefix('v').unwrap_or(version).to_string()
    }
}

pub(crate) fn check_version<V, T, F>(
    version_checker_id: String,
    version_checker: V,
    version_event_publisher: EventPublisher<T>,
    version_event_generator: F,
) where
    V: VersionChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(UpdatedAttributesMessage) -> T + Send + Sync + 'static,
{
    let span = info_span!(
        "version_check",
        { ID_ATTRIBUTE_NAME } = %version_checker_id
    );
    let _guard = span.enter();

    debug!("starting to check version with the configured checker");

    match version_checker.check_agent_version() {
        Ok(agent_data) => {
            info!("agent version successfully checked");

            publish_update_attributes_event(
                &version_event_publisher,
                version_event_generator(AgentDescription {
                    identifying_attributes: HashMap::from([(
                        agent_data.opamp_field,
                        agent_data.version.into(),
                    )]),
                    ..Default::default()
                }),
            );
        }
        Err(error) => {
            warn!("failed to check agent version: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::checkers::version::tests::MockVersionChecker;
    use crate::event::SubAgentInternalEvent;
    use crate::{
        agent_control::defaults::OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY,
        event::channel::pub_sub,
    };

    use super::*;

    use opamp_client::operation::settings::DescriptionValueType;
    use rstest::rstest;

    #[rstest]
    #[cfg_attr(
        target_family = "unix",
        case::command_and_regex("echo", vec!("Some".to_string(), "data".to_string(),"1.0.0".to_string()), Some(r"\d+\.\d+\.\d+")
        )
    )]
    #[cfg_attr(target_family = "unix", case::command("echo", vec!("-n".to_string(), "1.0.0".to_string()), None
    ))]
    #[cfg_attr(
        target_family = "windows",
        case::command_and_regex("cmd", vec!("/C".to_string(), "echo".to_string(), "Some".to_string(), "data".to_string(), "1.0.0".to_string()), Some(r"\d+\.\d+\.\d+"))
    )]
    #[cfg_attr(
        target_family = "windows",
        case::command("cmd", vec!("/C".to_string(),"set".to_string(),"/p=1.0.0<nul".to_string()), None)
    )]
    fn test_check_agent_version(
        #[case] path: &str,
        #[case] args: Vec<String>,
        #[case] regex: Option<&str>,
    ) {
        let agent_version = OnHostAgentVersionChecker {
            path: Some(path.to_string()),
            args: Some(Args(args)),
            regex: regex.map(|r| Regex::new(r).unwrap()),
            packages: None, // No packages, should use command
        }
        .check_agent_version()
        .unwrap();

        assert_eq!(agent_version.version.as_str(), "1.0.0");
        assert_eq!(
            agent_version.opamp_field.as_str(),
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
        );
    }

    #[test]
    fn test_check_version() {
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionChecker::new();
        version_checker
            .expect_check_agent_version()
            .once()
            .returning(move || {
                Ok(AgentVersion {
                    version: "1.0.0".to_string(),
                    opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                })
            });

        check_version(
            AgentID::default().to_string(),
            version_checker,
            version_publisher,
            SubAgentInternalEvent::AgentAttributesUpdated,
        );

        // Check that we received the expected version event
        assert_eq!(
            SubAgentInternalEvent::AgentAttributesUpdated(AgentDescription {
                identifying_attributes: HashMap::from([(
                    OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::String("1.0.0".to_string()),
                )]),
                ..Default::default()
            }),
            version_consumer.as_ref().recv().unwrap()
        );

        // Check there are no more events
        assert!(version_consumer.as_ref().recv().is_err());
    }

    mod package_version_tests {
        use super::*;
        use crate::agent_type::runtime_config::on_host::package::rendered::{
            Download, Oci, Package, Repository, Version,
        };
        use std::str::FromStr;

        #[test]
        fn test_version_from_package_tag() {
            let mut packages = HashMap::new();
            packages.insert(
                "ebpf-agent".to_string(),
                Package {
                    download: Download {
                        oci: Oci {
                            repository: Repository::from_str("newrelic/nr-ebpf-agent").unwrap(),
                            version: Version::from_str("0.5.2").unwrap(),
                            public_key_url: None,
                        },
                    },
                },
            );

            let checker = OnHostAgentVersionChecker {
                path: None,
                args: None,
                regex: None,
                packages: Some(packages),
            };

            let version = checker.check_agent_version().unwrap();
            assert_eq!(version.version, "0.5.2");
        }

        #[test]
        fn test_version_from_package_tag_with_v_prefix() {
            let mut packages = HashMap::new();
            packages.insert(
                "ebpf-agent".to_string(),
                Package {
                    download: Download {
                        oci: Oci {
                            repository: Repository::from_str("newrelic/nr-ebpf-agent").unwrap(),
                            version: Version::from_str("v7").unwrap(),
                            public_key_url: None,
                        },
                    },
                },
            );

            let checker = OnHostAgentVersionChecker {
                path: None,
                args: None,
                regex: None,
                packages: Some(packages),
            };

            let version = checker.check_agent_version().unwrap();
            assert_eq!(version.version, "7"); // 'v' prefix stripped
        }

        #[test]
        fn test_version_from_package_digest() {
            let mut packages = HashMap::new();
            packages.insert(
                "ebpf-agent".to_string(),
                Package {
                    download: Download {
                        oci: Oci {
                            repository: Repository::from_str("newrelic/nr-ebpf-agent").unwrap(),
                            version: Version::from_str(
                                "@sha256:ec5f08ee7be8b557cd1fc5ae1a0ac985e8538da7c93f51a51eff4b277509a723",
                            )
                            .unwrap(),
                            public_key_url: None,
                        },
                    },
                },
            );

            let checker = OnHostAgentVersionChecker {
                path: None,
                args: None,
                regex: None,
                packages: Some(packages),
            };

            let version = checker.check_agent_version().unwrap();
            assert_eq!(version.version, "digest-ec5f08ee7be8"); // Truncated digest
        }

        #[test]
        fn test_package_version_preferred_over_command() {
            let mut packages = HashMap::new();
            packages.insert(
                "agent".to_string(),
                Package {
                    download: Download {
                        oci: Oci {
                            repository: Repository::from_str("newrelic/agent").unwrap(),
                            version: Version::from_str("2.0.0").unwrap(),
                            public_key_url: None,
                        },
                    },
                },
            );

            #[cfg(target_family = "unix")]
            let checker = OnHostAgentVersionChecker {
                path: Some("echo".to_string()),
                args: Some(Args(vec!["-n".to_string(), "1.0.0".to_string()])),
                regex: None,
                packages: Some(packages),
            };

            #[cfg(target_family = "windows")]
            let checker = OnHostAgentVersionChecker {
                path: Some("cmd".to_string()),
                args: Some(Args(vec![
                    "/C".to_string(),
                    "set".to_string(),
                    "/p=1.0.0<nul".to_string(),
                ])),
                regex: None,
                packages: Some(packages),
            };

            let version = checker.check_agent_version().unwrap();
            // Should use package version (2.0.0) not command output (1.0.0)
            assert_eq!(version.version, "2.0.0");
        }

        #[test]
        fn test_command_fallback_when_no_packages() {
            #[cfg(target_family = "unix")]
            let checker = OnHostAgentVersionChecker {
                path: Some("echo".to_string()),
                args: Some(Args(vec!["-n".to_string(), "1.5.0".to_string()])),
                regex: None,
                packages: None,
            };

            #[cfg(target_family = "windows")]
            let checker = OnHostAgentVersionChecker {
                path: Some("cmd".to_string()),
                args: Some(Args(vec![
                    "/C".to_string(),
                    "set".to_string(),
                    "/p=1.5.0<nul".to_string(),
                ])),
                regex: None,
                packages: None,
            };

            let version = checker.check_agent_version().unwrap();
            assert_eq!(version.version, "1.5.0");
        }

        #[test]
        fn test_command_fallback_when_empty_packages() {
            let packages = HashMap::new(); // Empty

            #[cfg(target_family = "unix")]
            let checker = OnHostAgentVersionChecker {
                path: Some("echo".to_string()),
                args: Some(Args(vec!["-n".to_string(), "1.5.0".to_string()])),
                regex: None,
                packages: Some(packages),
            };

            #[cfg(target_family = "windows")]
            let checker = OnHostAgentVersionChecker {
                path: Some("cmd".to_string()),
                args: Some(Args(vec![
                    "/C".to_string(),
                    "set".to_string(),
                    "/p=1.5.0<nul".to_string(),
                ])),
                regex: None,
                packages: Some(packages),
            };

            let version = checker.check_agent_version().unwrap();
            assert_eq!(version.version, "1.5.0");
        }

        #[test]
        fn test_no_version_available() {
            let checker = OnHostAgentVersionChecker {
                path: None,
                args: None,
                regex: None,
                packages: None,
            };

            let result = checker.check_agent_version();
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .0
                .contains("no version source available"));
        }

        #[test]
        fn test_normalize_version() {
            assert_eq!(
                OnHostAgentVersionChecker::normalize_version("v1.2.3"),
                "1.2.3"
            );
            assert_eq!(
                OnHostAgentVersionChecker::normalize_version("1.2.3"),
                "1.2.3"
            );
            assert_eq!(OnHostAgentVersionChecker::normalize_version("v7"), "7");
            assert_eq!(
                OnHostAgentVersionChecker::normalize_version("latest"),
                "latest"
            );
        }
    }
}
