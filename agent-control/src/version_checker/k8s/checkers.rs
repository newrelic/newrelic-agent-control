use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta};
use crate::agent_type::version_config::{VersionCheckerInitialDelay, VersionCheckerInterval};
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::{get_namespace, get_type_meta};
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use crate::version_checker::k8s::helmrelease::HelmReleaseVersionChecker;
use crate::version_checker::k8s::instrumentation::NewrelicInstrumentationVersionChecker;
use crate::version_checker::{
    AgentVersion, VersionCheckError, VersionChecker, publish_version_event,
};
use kube::api::{DynamicObject, TypeMeta};
use std::sync::Arc;
use std::thread::sleep;
use tracing::{debug, info, info_span, warn};

use crate::version_checker::VERSION_CHECKER_THREAD_NAME;
use std::fmt::Debug;

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
        if type_meta == &instrumentation_v1beta1_type_meta() {
            return Ok(Self::Instrumentation);
        }
        Err(UnsupportedResourceType)
    }
}

/// Represents all supported version checkers for k8s objects.
#[cfg_attr(test, derive(Debug))]
pub enum K8sAgentVersionChecker {
    HelmRelease(HelmReleaseVersionChecker),
    Instrumentation(NewrelicInstrumentationVersionChecker),
}

impl VersionChecker for K8sAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        match self {
            K8sAgentVersionChecker::HelmRelease(vc) => vc.check_agent_version(),
            K8sAgentVersionChecker::Instrumentation(vc) => vc.check_agent_version(),
        }
    }
}

impl K8sAgentVersionChecker {
    /// Builds the VersionChecker corresponding to the first k8s object compatible with version check.
    /// It returns None if no object is compatible with version check.
    pub fn checked_new(
        k8s_client: Arc<SyncK8sClient>,
        agent_id: &AgentID,
        k8s_objects: Arc<Vec<DynamicObject>>,
        opamp_field: String,
    ) -> Option<Self> {
        // It returns the first version-checker matching an object.
        for object in k8s_objects.iter() {
            let Ok(namespace) = get_namespace(object) else {
                warn!("Skipping k8s object with empty namespace {:?}", object);
                continue;
            };
            let Ok(type_meta) = get_type_meta(object) else {
                warn!("Skipping k8s object with unknown type {:?}", object);
                continue;
            };
            let Ok(resource_type) = (&type_meta).try_into() else {
                continue;
            };

            let health_checker = match resource_type {
                SupportedResourceType::HelmRelease => {
                    Self::HelmRelease(HelmReleaseVersionChecker::new(
                        k8s_client,
                        type_meta,
                        namespace,
                        agent_id.to_string(),
                        opamp_field,
                    ))
                }
                SupportedResourceType::Instrumentation => {
                    Self::Instrumentation(NewrelicInstrumentationVersionChecker::new(
                        k8s_client, type_meta, namespace, agent_id,
                    ))
                }
            };
            return Some(health_checker);
        }
        warn!(
            "Version cannot be fetched from any of the agent underlying resources, it won't be reported"
        );
        None
    }
}

pub(crate) fn spawn_version_checker<V, T, F>(
    version_checker_id: String,
    version_checker: V,
    version_event_publisher: EventPublisher<T>,
    version_event_generator: F,
    interval: VersionCheckerInterval,
    initial_delay: VersionCheckerInitialDelay,
) -> StartedThreadContext
where
    V: VersionChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(AgentVersion) -> T + Send + Sync + 'static,
{
    let thread_name = format!("{version_checker_id}_{VERSION_CHECKER_THREAD_NAME}");
    // Stores if the version was retrieved in last iteration for logging purposes.
    let mut version_retrieved = false;
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        let span = info_span!(
            "version_check",
            { ID_ATTRIBUTE_NAME } = %version_checker_id
        );
        let _guard = span.enter();

        debug!("starting to check version with the configured checker");

        sleep(initial_delay.into());

        match version_checker.check_agent_version() {
            Ok(agent_data) => {
                if !version_retrieved {
                    info!("agent version successfully checked");
                    version_retrieved = true;
                }

                publish_version_event(
                    &version_event_publisher,
                    version_event_generator(agent_data),
                );
            }
            Err(error) => {
                warn!("failed to check agent version: {error}");
                version_retrieved = false;
            }
        }

        if stop_consumer.is_cancelled(interval.into()) {
            break;
        }
    };

    NotStartedThreadContext::new(thread_name, callback).start()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version_checker::k8s::checkers::tests::SubAgentInternalEvent::AgentVersionInfo;
    use crate::{
        agent_control::{
            config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta},
            defaults::OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY,
        },
        event::{SubAgentInternalEvent, channel::pub_sub},
        k8s::client::MockSyncK8sClient,
    };
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::{DynamicObject, TypeMeta};
    use mockall::{Sequence, mock};
    use std::{sync::Arc, time::Duration};

    mock! {
        pub VersionChecker {}
        impl VersionChecker for VersionChecker {
            fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
        }
    }

    #[test]
    fn test_agent_version_checker_build() {
        struct TestCase {
            name: &'static str,
            k8s_objects: Vec<DynamicObject>,
            check: fn(&'static str, Option<K8sAgentVersionChecker>),
        }

        impl TestCase {
            fn run(self) {
                let k8s_objects = Arc::new(self.k8s_objects);
                let result = K8sAgentVersionChecker::checked_new(
                    Arc::new(MockSyncK8sClient::new()),
                    &AgentID::try_from("some-agent-id").unwrap(),
                    k8s_objects,
                    OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
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
                    assert_matches!(
                        result,
                        Some(K8sAgentVersionChecker::HelmRelease(_)),
                        "{name}",
                    );
                },
            },
            TestCase {
                name: "Instrumentation object",
                k8s_objects: [instrumentation_dyn_obj()].to_vec(),
                check: |name, result| {
                    assert_matches!(
                        result,
                        Some(K8sAgentVersionChecker::Instrumentation(_)),
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
                    assert_matches!(
                        result,
                        Some(K8sAgentVersionChecker::HelmRelease(_)),
                        "{name}",
                    );
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
                        Some(K8sAgentVersionChecker::Instrumentation(_)),
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
            metadata: ObjectMeta {
                name: Some("some-name".to_string()),
                namespace: Some("some-namespace".to_string()),
                ..Default::default()
            },
            data: Default::default(),
        }
    }

    fn helm_release_dyn_obj() -> DynamicObject {
        empty_dynamic_object(helmrelease_v2_type_meta())
    }

    fn instrumentation_dyn_obj() -> DynamicObject {
        empty_dynamic_object(instrumentation_v1beta1_type_meta())
    }

    fn secret_dyn_obj() -> DynamicObject {
        empty_dynamic_object(TypeMeta {
            api_version: "v1".into(),
            kind: "Secret".into(),
        })
    }

    #[test]
    fn test_spawn_version_checker() {
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionChecker::new();
        let mut seq = Sequence::new();
        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || Err(VersionCheckError("mocked version check error!".to_string())));
        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(AgentVersion {
                    version: "1.0.0".to_string(),
                    opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                })
            });

        let started_thread_context = spawn_version_checker(
            AgentID::default().to_string(),
            version_checker,
            version_publisher,
            SubAgentInternalEvent::AgentVersionInfo,
            Duration::from_millis(10).into(),
            Duration::from_millis(500).into(),
        );

        // Check we didn't receive anything too early
        sleep(Duration::from_millis(300));
        assert!(version_consumer.as_ref().is_empty());

        // Check that we received the expected version event
        assert_eq!(
            AgentVersionInfo(AgentVersion {
                version: "1.0.0".to_string(),
                opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            }),
            version_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(version_consumer.as_ref().recv().is_err());
    }
}
