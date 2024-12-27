mod helmrelease;
mod instrumentation;

use crate::agent_control::config::AgentID;
use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1alpha2_type_meta};
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use helmrelease::HelmReleaseVersionChecker;
use instrumentation::NewrelicInstrumentationVersionChecker;
use kube::api::{DynamicObject, TypeMeta};
use std::sync::Arc;
use std::thread;
use tracing::{debug, error, warn};

/// Represents the k8s resource types supporting version check.
enum SupportedResourceType {
    HelmRelease,
    Instrumentation,
}

/// Type representing all k8s resource types not supporting version check.
struct UnsupportedResourceType;

impl TryFrom<&TypeMeta> for SupportedResourceType {
    type Error = UnsupportedResourceType;

    fn try_from(type_meta: &TypeMeta) -> Result<Self, Self::Error> {
        if type_meta == &helmrelease_v2_type_meta() {
            return Ok(Self::HelmRelease);
        }
        if type_meta == &instrumentation_v1alpha2_type_meta() {
            return Ok(Self::Instrumentation);
        }
        Err(UnsupportedResourceType)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentVersion {
    version: String,
    opamp_field: String,
}

impl AgentVersion {
    pub fn new(version: String, opamp_field: String) -> Self {
        Self {
            version,
            opamp_field,
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn opamp_field(&self) -> &str {
        &self.opamp_field
    }
}

#[derive(thiserror::Error, Debug)]
pub enum VersionCheckError {
    #[error("Generic error: {0}")]
    Generic(String),
}
pub trait VersionChecker {
    /// Use it to report the agent version for the opamp client
    /// Uses a thread to check the version of and agent and report it
    /// with internal events. The reported AgentVersion should
    /// contain "version" and the field for opamp that is going to contain the version
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
}

pub(crate) fn spawn_version_checker<V>(
    agent_id: AgentID,
    version_checker: V,
    cancel_signal: EventConsumer<CancellationMessage>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: VersionCheckerInterval,
) where
    V: VersionChecker + Send + Sync + 'static,
{
    thread::spawn(move || loop {
        if cancel_signal.is_cancelled(interval.into()) {
            break;
        }
        debug!(%agent_id, "starting to check version with the configured checker");

        match version_checker.check_agent_version() {
            Ok(agent_data) => {
                let event = SubAgentInternalEvent::AgentVersionInfo(agent_data);
                _ = sub_agent_internal_publisher
                    .publish(event.clone())
                    .inspect_err(|e| {
                        error!(
                            err = e.to_string(),
                            event_type = format!("{:?}", event),
                            "could not publish sub agent event"
                        )
                    })
            }
            Err(error) => {
                error!(%agent_id, %error, "failed to check agent version");
            }
        }
    });
}

/// Represents all supported version checkers for k8s objects.
#[cfg_attr(test, derive(Debug))]
pub enum AgentVersionChecker {
    HelmRelease(HelmReleaseVersionChecker),
    Instrumentation(NewrelicInstrumentationVersionChecker),
}

impl VersionChecker for AgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        match self {
            AgentVersionChecker::HelmRelease(vc) => vc.check_agent_version(),
            AgentVersionChecker::Instrumentation(vc) => vc.check_agent_version(),
        }
    }
}

impl AgentVersionChecker {
    /// Builds the VersionChecker corresponding to the first k8s object compatible with version check.
    /// It returns None if no object is compatible with version check.
    pub fn checked_new(
        k8s_client: Arc<SyncK8sClient>,
        agent_id: String,
        k8s_objects: Arc<Vec<DynamicObject>>,
    ) -> Option<Self> {
        // It returns the first version-checker matching an object.
        for object in k8s_objects.iter() {
            let Some(type_meta) = object.types.clone() else {
                warn!(%agent_id, "Skipping k8s object with unknown type {:?}", object);
                continue;
            };
            let Ok(resource_type) = (&type_meta).try_into() else {
                continue;
            };
            let health_checker = match resource_type {
                SupportedResourceType::HelmRelease => Self::HelmRelease(
                    HelmReleaseVersionChecker::new(k8s_client, type_meta, agent_id),
                ),
                SupportedResourceType::Instrumentation => Self::Instrumentation(
                    NewrelicInstrumentationVersionChecker::new(k8s_client, type_meta, agent_id),
                ),
            };
            return Some(health_checker);
        }
        warn!(%agent_id, "Version cannot be fetched from any of the agent underlying resources, it won't be reported");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::AgentID;
    use crate::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
    use crate::event::channel::pub_sub;
    use crate::event::SubAgentInternalEvent;
    use crate::event::SubAgentInternalEvent::AgentVersionInfo;
    use crate::{
        agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1alpha2_type_meta},
        k8s::client::MockSyncK8sClient,
    };
    use assert_matches::assert_matches;
    use kube::api::{DynamicObject, TypeMeta};
    use mockall::{mock, Sequence};
    use std::sync::Arc;
    use std::time::Duration;

    mock! {
        pub VersionCheckerMock {}
        impl VersionChecker for VersionCheckerMock {
            fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
        }
    }

    #[test]
    fn test_spawn_version_checker() {
        let (cancel_publisher, cancel_signal) = pub_sub();
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionCheckerMock::new();
        let mut seq = Sequence::new();
        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(AgentVersion::new(
                    "1.0.0".to_string(),
                    OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                ))
            });

        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                cancel_publisher.publish(()).unwrap();
                Err(VersionCheckError::Generic(
                    "mocked version check error!".to_string(),
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        spawn_version_checker(
            agent_id,
            version_checker,
            cancel_signal,
            version_publisher,
            Duration::default().into(),
        );

        let expected_version_events: Vec<SubAgentInternalEvent> = {
            vec![AgentVersionInfo(AgentVersion {
                version: "1.0.0".to_string(),
                opamp_field: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            })]
        };
        let actual_version_events = version_consumer.as_ref().iter().collect::<Vec<_>>();
        assert_eq!(expected_version_events, actual_version_events);
    }

    #[test]
    fn test_agent_version_checker_build() {
        struct TestCase {
            name: &'static str,
            k8s_objects: Vec<DynamicObject>,
            check: fn(&'static str, Option<AgentVersionChecker>),
        }

        impl TestCase {
            fn run(self) {
                let k8s_objects = Arc::new(self.k8s_objects);
                let result = AgentVersionChecker::checked_new(
                    Arc::new(MockSyncK8sClient::new()),
                    "some-agent-id".into(),
                    k8s_objects,
                );
                let check = self.check;
                check(self.name, result);
            }
        }

        let test_cases = [
            TestCase {
                name: "HelmRelease object",
                k8s_objects: [helm_release_dyn_obj()].to_vec(),
                check: |name, result| {
                    assert_matches!(result, Some(AgentVersionChecker::HelmRelease(_)), "{name}",);
                },
            },
            TestCase {
                name: "Instrumentation object",
                k8s_objects: [instrumentation_dyn_obj()].to_vec(),
                check: |name, result| {
                    assert_matches!(
                        result,
                        Some(AgentVersionChecker::Instrumentation(_)),
                        "{name}"
                    );
                },
            },
            TestCase {
                name: "Unsupported object",
                k8s_objects: [secret_dyn_obj()].to_vec(),
                check: |name, result| assert!(result.is_none(), "{name}"),
            },
            TestCase {
                name: "HelmRelease first",
                k8s_objects: [
                    secret_dyn_obj(),
                    helm_release_dyn_obj(),
                    instrumentation_dyn_obj(),
                ]
                .to_vec(),
                check: |name, result| {
                    assert_matches!(result, Some(AgentVersionChecker::HelmRelease(_)), "{name}",);
                },
            },
            TestCase {
                name: "Instrumentation first",
                k8s_objects: [
                    secret_dyn_obj(),
                    instrumentation_dyn_obj(),
                    helm_release_dyn_obj(),
                ]
                .to_vec(),
                check: |name, result| {
                    assert_matches!(
                        result,
                        Some(AgentVersionChecker::Instrumentation(_)),
                        "{name}",
                    );
                },
            },
            TestCase {
                name: "No objects",
                k8s_objects: Vec::new(),
                check: |name, result| assert!(result.is_none(), "{name}"),
            },
            TestCase {
                name: "Invalid dynamic object",
                k8s_objects: [DynamicObject {
                    types: None,
                    metadata: Default::default(),
                    data: Default::default(),
                }]
                .to_vec(),
                check: |name, result| assert!(result.is_none(), "{name}"),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    fn empty_dynamic_object(type_meta: TypeMeta) -> DynamicObject {
        DynamicObject {
            types: Some(type_meta),
            metadata: Default::default(),
            data: Default::default(),
        }
    }

    fn helm_release_dyn_obj() -> DynamicObject {
        empty_dynamic_object(helmrelease_v2_type_meta())
    }

    fn instrumentation_dyn_obj() -> DynamicObject {
        empty_dynamic_object(instrumentation_v1alpha2_type_meta())
    }

    fn secret_dyn_obj() -> DynamicObject {
        empty_dynamic_object(TypeMeta {
            api_version: "v1".into(),
            kind: "Secret".into(),
        })
    }
}
