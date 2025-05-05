use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::version::k8s::helmrelease::HelmReleaseVersionChecker;
use crate::sub_agent::version::k8s::instrumentation::NewrelicInstrumentationVersionChecker;
use crate::sub_agent::version::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use kube::api::{DynamicObject, TypeMeta};
use std::sync::Arc;
use tracing::warn;

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
    ) -> Option<Self> {
        // It returns the first version-checker matching an object.
        for object in k8s_objects.iter() {
            let Some(type_meta) = object.types.clone() else {
                warn!("Skipping k8s object with unknown type {:?}", object);
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
        warn!(
            "Version cannot be fetched from any of the agent underlying resources, it won't be reported"
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta},
        k8s::client::MockSyncK8sClient,
    };
    use assert_matches::assert_matches;
    use kube::api::{DynamicObject, TypeMeta};
    use mockall::mock;
    use std::sync::Arc;

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
                    &AgentID::new("some-agent-id").unwrap(),
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
            metadata: Default::default(),
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
}
