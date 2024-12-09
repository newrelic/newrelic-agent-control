#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::version::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use chrono::NaiveDateTime;
use serde_json::Value;
use std::sync::Arc;

const LAST_ATTEMPTED_REVISION: &str = "lastAttemptedRevision";
const LAST_REVISION: &str = "*";
pub struct K8sVersionChecker {
    k8s_client: Arc<SyncK8sClient>,
    name: String,
}

impl K8sVersionChecker {
    pub fn new(k8s_client: Arc<SyncK8sClient>, name: String) -> Self {
        Self { k8s_client, name }
    }
    fn extract_version(
        &self,
        data: &serde_json::Map<String, Value>,
    ) -> Result<AgentVersion, VersionCheckError> {
        println!("NAME TO CHECK {:?}",self.name);
        let extractors = [
            extract_revision,
            extract_last_deployed_revision,
            extract_revision_from_history,
        ];

        for extractor in &extractors {
            if let Ok(version) = extractor(data) {
                if !version.is_empty() {
                    return Ok(AgentVersion::new(version));
                }
            }
        }

        Err(VersionCheckError::Generic(
            "No valid version found in HelmRelease".to_string(),
        ))
    }
}

impl VersionChecker for K8sVersionChecker {
    fn check_version(&self) -> Result<AgentVersion, VersionCheckError> {
        println!("NAME TO CHECK 1: {:?}",self.name);
        // Attempt to get the HelmRelease from Kubernetes
        let helm_release = self
            .k8s_client
            .get_helm_release(&self.name)
            .map_err(|e| {
                VersionCheckError::Generic(format!(
                    "Error fetching HelmRelease '{}': {}",
                    &self.name, e
                ))
            })?
            .ok_or_else(|| {
                VersionCheckError::Generic(format!("HelmRelease '{}' not found", &self.name))
            })?;

        let helm_release_data = helm_release.data.as_object().ok_or_else(|| {
            VersionCheckError::Generic("HelmRelease data is not an object".to_string())
        })?;
        if let Ok(version) = extract_revision(helm_release_data) {
            if !version.is_empty() {
                return Ok(AgentVersion::new(version));
            }
        }

        Ok(self
            .extract_version(helm_release_data)
            .expect("No valid version found in HelmRelease"))
    }
}
//Attempt to get version from chart
fn extract_revision(
    helm_data: &serde_json::map::Map<String, Value>,
) -> Result<String, VersionCheckError> {
    helm_data
        .get("spec")
        .and_then(|spec| spec.get("chart"))
        .and_then(|chart| chart.get("spec"))
        .and_then(|spec| spec.get("version"))
        .and_then(|version| version.as_str())
        .filter(|&version| version != LAST_REVISION)
        .map(|version| version.to_string())
        .ok_or_else(|| VersionCheckError::Generic("revision not found".to_string()))
}
//Attempt to get version from last attempted deployed revision
fn extract_last_deployed_revision(
    helm_data: &serde_json::map::Map<String, Value>,
) -> Result<String, VersionCheckError> {
    helm_data
        .get(LAST_ATTEMPTED_REVISION)
        .and_then(|last_attempt_revision| last_attempt_revision.as_str())
        .filter(|version| !version.is_empty())
        .map(|version| version.to_string())
        .ok_or_else(|| VersionCheckError::Generic("last attempted revision not found".to_string()))
}
//Attempt to get version from the history looking for status deployed and sort by date
fn extract_revision_from_history(
    helm_data: &serde_json::map::Map<String, Value>,
) -> Result<String, VersionCheckError> {
    let helm_history = helm_data
        .get("history")
        .and_then(|history| history.as_array());

    if helm_history.is_none() {
        return Err(VersionCheckError::Generic(
            "history for revision not found".into(),
        ));
    }

    let history_entries = helm_history.unwrap();

    let latest_entry = history_entries
        .iter()
        .filter_map(|history_item| {
            let item = history_item.as_object()?;
            let status = item.get("status")?.as_str()?;
            let deployment_date = item.get("firstDeployed")?.as_str()?;
            let chart_version = item.get("chartVersion")?.as_str()?;

            if status == "deployed" {
                let parsed_date =
                    NaiveDateTime::parse_from_str(deployment_date, "%Y-%m-%dT%H:%M:%SZ").ok()?;
                Some((parsed_date, chart_version.to_string()))
            } else {
                None
            }
        })
        .max_by_key(|entry| entry.0);

    match latest_entry {
        Some((_, version)) => Ok(version),
        None => Err(VersionCheckError::Generic("revision not found".into())),
    }
}
#[cfg(test)]
pub mod test {
    use crate::k8s::client::MockSyncK8sClient;
    use crate::sub_agent::version::k8s::k8s_version_checker::K8sVersionChecker;
    use crate::sub_agent::version::version_checker::{AgentVersion, VersionChecker};
    use crate::super_agent::config::helm_release_type_meta;
    use kube::api::DynamicObject;
    use serde_json::{json, Value};
    use std::sync::Arc;

    #[test]
    fn given_a_chart_read_the_version_from_chart_version() {
        let mut mock_client = MockSyncK8sClient::new();

        setup_mock_client(
            &mut mock_client,
            get_dynamic_object(build_json_data("1.12.12", "1.15.1")),
        );
        let checker = K8sVersionChecker::new(Arc::new(mock_client), String::from("default-test"));
        let version = checker.check_version().unwrap();
        assert_eq!(version, AgentVersion::new(String::from("1.12.12")));
    }

    #[test]
    fn given_a_chart_read_the_version_from_last_attempted_revision() {
        let mut mock_client = MockSyncK8sClient::new();
        setup_mock_client(
            &mut mock_client,
            get_dynamic_object(build_json_data("*", "1.15.1")),
        );
        let checker = K8sVersionChecker::new(Arc::new(mock_client), String::from("default-test"));
        let version = checker.check_version().unwrap();
        assert_eq!(version, AgentVersion::new(String::from("1.15.1")));
    }

    #[test]
    fn given_a_chart_read_the_version_from_the_history() {
        let mut mock_client = MockSyncK8sClient::new();
        setup_mock_client(
            &mut mock_client,
            get_dynamic_object(build_json_data("*", "")),
        );
        let checker = K8sVersionChecker::new(Arc::new(mock_client), String::from("default-test"));
        let version = checker.check_version().unwrap();
        assert_eq!(version, AgentVersion::new(String::from("1.43.6")));
    }

    fn setup_mock_client(mock: &mut MockSyncK8sClient, expected_response: DynamicObject) {
        mock.expect_get_helm_release()
            .withf(|name| name == "default-test")
            .times(1)
            .returning(move |_| Ok(Some(Arc::new(expected_response.clone()))));
        mock.expect_has_dynamic_object_changed()
            .returning(|_| Ok(false));
    }

    fn build_json_data(chart_version: &str, last_attempted_version: &str) -> String {
        format!(
            r#"{{
        "lastAttemptedRevision": "{}",
        "spec": {{
            "chart": {{
                "spec": {{
                    "chart": "default-test",
                    "version": "{}"
                }}
            }}
        }},
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
    }}"#,
            last_attempted_version, chart_version
        )
    }
    fn get_dynamic_object(json_data: String) -> DynamicObject {
        let parsed_data: Value = serde_json::from_str(&json_data).expect("Error parsing JSON");
        DynamicObject {
            types: Some(helm_release_type_meta()),
            metadata: Default::default(),
            data: json!(parsed_data),
        }
    }
}
