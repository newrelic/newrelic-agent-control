use crate::agent_control::config::AgentID;
use crate::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::version::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use chrono::NaiveDateTime;
use kube::api::TypeMeta;
use serde_json::{Map, Value};
use std::sync::Arc;
use tracing::debug;

const LATEST_REVISION: &str = "*";

pub struct HelmReleaseVersionChecker {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    agent_id: AgentID,
}

impl HelmReleaseVersionChecker {
    pub fn new(k8s_client: Arc<SyncK8sClient>, type_meta: TypeMeta, agent_id: &AgentID) -> Self {
        Self {
            k8s_client,
            type_meta,
            agent_id: agent_id.clone(),
        }
    }
    fn extract_version(
        &self,
        data: &Map<String, Value>,
    ) -> Result<AgentVersion, VersionCheckError> {
        let extractors = [from_version, from_last_deployed, from_history];

        for extractor in &extractors {
            if let Some(version) = extractor(data, &self.agent_id) {
                if !version.is_empty() {
                    return Ok(AgentVersion::new(
                        version,
                        OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                    ));
                }
            }
        }

        Err(VersionCheckError::Generic(
            "No valid version found in HelmRelease".to_string(),
        ))
    }
}

impl VersionChecker for HelmReleaseVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        // Attempt to get the HelmRelease from Kubernetes
        let helm_release = self
            .k8s_client
            .get_dynamic_object(&self.type_meta, &self.agent_id)
            .map_err(|e| {
                VersionCheckError::Generic(format!(
                    "Error fetching HelmRelease '{}': {}",
                    &self.agent_id, e
                ))
            })?
            .ok_or_else(|| {
                VersionCheckError::Generic(format!("HelmRelease '{}' not found", &self.agent_id))
            })?;

        let helm_release_data = helm_release.data.as_object().ok_or_else(|| {
            VersionCheckError::Generic("HelmRelease data is not an object".to_string())
        })?;

        self.extract_version(helm_release_data)
    }
}

//Attempt to get version from chart
fn from_version(helm_data: &Map<String, Value>, agent_id: &AgentID) -> Option<String> {
    let version = helm_data
        .get("spec")?
        .get("chart")?
        .get("spec")?
        .get("version")?
        // The as_str is needed, using directly the to_string will add an extra \"\"
        .as_str()?
        .to_string();

    if version.contains(LATEST_REVISION) {
        return None;
    }
    debug!(%agent_id, %version, "version extracted from version");
    Some(version)
}

//Attempt to get version from last attempted deployed revision
fn from_last_deployed(helm_data: &Map<String, Value>, agent_id: &AgentID) -> Option<String> {
    let version = helm_data
        .get("status")?
        .get("lastAttemptedRevision")?
        // The as_str is needed, using directly the to_string will add an extra \"\"
        .as_str()?
        .to_string();

    if version.is_empty() {
        return None;
    }
    debug!(%agent_id,%version, "version extracted from lastAttemptedRevision");
    Some(version)
}

//Attempt to get version from the history looking for status deployed and sort by date
fn from_history(helm_data: &Map<String, Value>, agent_id: &AgentID) -> Option<String> {
    let helm_history = helm_data.get("status")?.get("history")?.as_array()?;

    let (_, version) = helm_history
        .iter()
        .filter_map(|item| {
            if item.get("status")?.as_str()? != "deployed" {
                return None;
            }
            // The as_str is needed, using directly the to_string will add an extra \"\"
            let chart_version = item.get("chartVersion")?.as_str()?.to_string();
            let deployment_date = item.get("firstDeployed")?.as_str()?;
            let parsed_date =
                NaiveDateTime::parse_from_str(deployment_date, "%Y-%m-%dT%H:%M:%SZ").ok()?;
            Some((parsed_date, chart_version))
        })
        .max_by_key(|entry| entry.0)?;

    debug!(%agent_id, %version, "version extracted from history");
    Some(version)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
    use crate::k8s::client::MockSyncK8sClient;
    use kube::api::DynamicObject;
    use serde_json::{json, Value};
    use std::sync::Arc;

    impl std::fmt::Debug for HelmReleaseVersionChecker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "HelmReleaseVersionChecker{{agent_id: {}}}",
                self.agent_id
            )
        }
    }

    #[test]
    fn test_k8s_check_agent_version() {
        struct TestCase {
            name: &'static str,
            expected: Result<AgentVersion, VersionCheckError>,
            mock_return: String,
        }
        impl TestCase {
            fn run(self) {
                let mut k8s_client = MockSyncK8sClient::new();
                setup_default_mock(&mut k8s_client, self.mock_return);
                let check = HelmReleaseVersionChecker::new(
                    Arc::new(k8s_client),
                    helmrelease_v2_type_meta(),
                    &AgentID::new("default-test").unwrap(),
                );
                let result = check.check_agent_version();
                match self.expected {
                    Ok(expected_agent_version) => {
                        let agent_version_result = result.unwrap_or_else(|e| {
                            panic!("Failed to check agent version {}: {}", self.name, e)
                        });
                        assert_eq!(expected_agent_version, agent_version_result);
                    }
                    Err(expected_err) => {
                        assert_eq!(
                            expected_err.to_string(),
                            format!("{}", result.unwrap_err()),
                            "{}",
                            self.name
                        );
                    }
                }
            }
        }
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                name: "Helm version is obtained from the chart version",
                expected: Ok(AgentVersion::new(
                    String::from("1.12.12"),
                    OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                )),
                mock_return: build_json_data("1.12.12", "1.15.1"),
            },
            TestCase {
                name: "Helm version is obtained from the last attempted revision",
                expected: Ok(AgentVersion::new(
                    String::from("1.15.1"),
                    OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                )),
                mock_return: build_json_data("*", "1.15.1"),
            },
            TestCase {
                name: "Helm version is obtained from the history",
                expected: Ok(AgentVersion::new(
                    String::from("1.43.6"),
                    OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                )),
                mock_return: build_json_data("*", ""),
            },
            TestCase {
                name: "Helm version couldn't be obtained from the helm data",
                expected: Err(VersionCheckError::Generic(
                    "No valid version found in HelmRelease".to_string(),
                )),
                mock_return: "{}".to_string(),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
    fn build_json_data(chart_version: &str, last_attempted_version: &str) -> String {
        format!(
            r#"{{
        "spec": {{
            "chart": {{
                "spec": {{
                    "chart": "default-test",
                    "version": "{}"
                }}
            }}
        }},
        "status": {{
            "lastAttemptedRevision": "{}",
            "history": [
                {{
                    "chartName": "default-test",
                    "chartVersion": "1.45.6",
                    "firstDeployed": "2024-11-13T14:28:33Z",
                    "status": "deployed"
                }},
                {{
                    "chartName": "default-test",
                    "chartVersion": "1.43.6",
                    "firstDeployed": "2024-11-16T14:28:33Z",
                    "status": "deployed"
                }},
                {{
                    "chartName": "default-test",
                    "chartVersion": "1.45.9",
                    "firstDeployed": "2024-11-14T14:28:33Z",
                    "status": "fail"
                }}
            ]
        }}    
    }}"#,
            chart_version, last_attempted_version
        )
    }
    fn get_dynamic_object(json_data: String) -> DynamicObject {
        let parsed_data: Value = serde_json::from_str(&json_data).expect("Error parsing JSON");
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: Default::default(),
            data: json!(parsed_data),
        }
    }

    fn setup_default_mock(mock: &mut MockSyncK8sClient, json_data: String) {
        mock.expect_get_dynamic_object()
            .withf(|type_meta, name| {
                type_meta == &helmrelease_v2_type_meta() && name == "default-test"
            })
            .times(1)
            .returning(move |_, _| Ok(Some(Arc::new(get_dynamic_object(json_data.clone())))));
    }
}
