use crate::agent_control::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::version_checker::{AgentVersion, VersionChecker};
use crate::{agent_control::agent_id::AgentID, version_checker::VersionCheckError};
use kube::api::{DynamicObject, TypeMeta};
use std::sync::Arc;

pub struct NewrelicInstrumentationVersionChecker {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    namespace: String,
    agent_id: AgentID,
}

impl NewrelicInstrumentationVersionChecker {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        type_meta: TypeMeta,
        namespace: String,
        agent_id: &AgentID,
    ) -> Self {
        Self {
            k8s_client,
            type_meta,
            namespace,
            agent_id: agent_id.clone(),
        }
    }

    fn get_instrumentation(&self) -> Result<Arc<DynamicObject>, VersionCheckError> {
        self.k8s_client
            .get_dynamic_object(
                &self.type_meta,
                self.agent_id.as_str(),
                self.namespace.as_str(),
            )
            .map_err(|err| {
                VersionCheckError(format!(
                    "Error fetching Instrumentation for agent_id '{}': {}",
                    &self.agent_id, err
                ))
            })?
            .ok_or_else(|| {
                VersionCheckError(format!(
                    "Instrumentation for agent_id '{}' not found",
                    &self.agent_id
                ))
            })
    }
}

impl VersionChecker for NewrelicInstrumentationVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        let instrumentation = self.get_instrumentation()?;

        let instrumentation_data = instrumentation.data.as_object().ok_or_else(|| {
            VersionCheckError(format!(
                "Invalid Instrumentation for agent_id '{}'",
                &self.agent_id
            ))
        })?;

        let version = version_from_newrelic_instrumentation_image(instrumentation_data)
            .ok_or_else(|| {
                VersionCheckError(format!(
                    "Could not extract version from 'spec.agent.image' in the Instrumentation object for '{}'",
                    &self.agent_id
                ))
            })?;

        let agent_version = AgentVersion {
            version,
            opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        };

        Ok(agent_version)
    }
}

/// Obtains the version from the data of a 'newrelic instrumentation' (newrelic.com/v1beta1, Instrumentation) object.
/// Specifically it gets it from `spec.agent.image`, where the image's tag is considered the version.
fn version_from_newrelic_instrumentation_image(
    data: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    data.get("spec")
        .and_then(|spec| spec.get("agent"))
        .and_then(|agent| agent.get("image"))
        .and_then(|image| image.as_str())
        .and_then(|image| image.split(":").nth(1).map(|version| version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    impl std::fmt::Debug for NewrelicInstrumentationVersionChecker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "NewrelicInstrumentationVersionChecker{{agent_id: {}}}",
                self.agent_id,
            )
        }
    }

    #[test]
    fn test_version_from_newrelic_instrumentation_image() {
        struct TestCase {
            name: &'static str,
            instrumentation_json: serde_json::Value,
            expected: Option<String>,
        }

        impl TestCase {
            fn run(self) {
                let spec = self
                    .instrumentation_json
                    .as_object()
                    .unwrap_or_else(|| panic!("Invalid json for test case '{}'", self.name));
                let result = version_from_newrelic_instrumentation_image(spec);
                assert_eq!(result, self.expected, "Test '{}' failed", self.name);
            }
        }

        let test_cases = [
            TestCase {
                name: "No spec",
                instrumentation_json: json!({}),
                expected: None,
            },
            TestCase {
                name: "No agent",
                instrumentation_json: json!({"spec": {}}),
                expected: None,
            },
            TestCase {
                name: "No image",
                instrumentation_json: json!({"spec": {"agent": {}}}),
                expected: None,
            },
            TestCase {
                name: "No tag",
                instrumentation_json: json!({"spec": {"agent": {"image": "some-image"}}}),
                expected: None,
            },
            TestCase {
                name: "Image with tag",
                instrumentation_json: json!({"spec": {"agent": {"image": "some-image:latest"}}}),
                expected: Some("latest".to_string()),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
