use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy, Unhealthy};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::TypeMeta;
use lazy_static::lazy_static;
use regex::Regex;
use std::string::ToString;
use std::sync::Arc;

lazy_static! {
    static ref CRD_TYPEMETA: TypeMeta = TypeMeta {
        api_version: "apiextensions.k8s.io/v1".to_string(),
        kind: "CustomResourceDefinition".to_string(),
    };
}

const EXPECTED_CRDS: &[&str] = &[
    "buckets.source.toolkit.fluxcd.io",
    "gitrepositories.source.toolkit.fluxcd.io",
    "helmcharts.source.toolkit.fluxcd.io",
    "helmreleases.helm.toolkit.fluxcd.io",
    "helmrepositories.source.toolkit.fluxcd.io",
    "ocirepositories.source.toolkit.fluxcd.io",
];

/// Enumerates the possible statuses that a Kubernetes condition can report.
#[derive(Debug, PartialEq, Eq)]
enum ConditionStatus {
    True,
    False,
    Unknown,
}

impl From<&str> for ConditionStatus {
    fn from(s: &str) -> Self {
        match s {
            "True" => ConditionStatus::True,
            "False" => ConditionStatus::False,
            _ => ConditionStatus::Unknown,
        }
    }
}

/// Represents a health checker for a specific HelmRelease in Kubernetes.
///
/// This struct is designed to be used within a wrapper that manages multiple
/// instances, each corresponding to a different HelmRelease, allowing for
/// health checks across several Helm releases within a Kubernetes cluster.
#[derive(Debug)]
pub struct FluxCrdsHealthChecker {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    expected_version: String,
    start_time: StartTime,
}

impl FluxCrdsHealthChecker {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        type_meta: TypeMeta,
        expected_version: String,
        start_time: StartTime,
    ) -> Self {
        Self {
            k8s_client,
            type_meta,
            expected_version,
            start_time,
        }
    }

    /// Checks that Flux CRDs exist and have the expected version label.
    fn check_crds(&self) -> Result<(), HealthCheckerError> {
        let re = Regex::new(r"\d+\.\d+\.\d+").unwrap();

        for &crd_name in EXPECTED_CRDS {
            let crd = self
                .k8s_client
                .get_dynamic_object(&self.type_meta, crd_name, "")
                .map_err(|e| {
                    HealthCheckerError::Generic(format!("Error checking CRD {crd_name}: {e}"))
                })?
                .ok_or_else(|| {
                    HealthCheckerError::Generic(format!("Flux CRD not found: {crd_name}"))
                })?;

            let labels = crd.metadata.labels.as_ref().ok_or_else(|| {
                HealthCheckerError::Generic(format!("CRD '{crd_name}' has no labels"))
            })?;

            let version_label = labels.get("helm.sh/chart").ok_or_else(|| {
                HealthCheckerError::Generic(format!(
                    "CRD '{crd_name}' is missing the 'helm.sh/chart' label"
                ))
            })?;

            let found_version = re.find(version_label).map(|mat| mat.as_str()).ok_or_else(
                || {
                    HealthCheckerError::Generic(format!(
                        "Could not find a version pattern in label for CRD '{crd_name}'. Full label: {version_label}"
                    ))
                },
            )?;

            if found_version != self.expected_version {
                return Err(HealthCheckerError::Generic(format!(
                    "Incorrect version for CRD '{}'. Expected: {}, Found: {}",
                    crd_name, self.expected_version, found_version
                )));
            }
        }
        Ok(())
    }
}

impl HealthChecker for FluxCrdsHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        if let Err(e) = self.check_crds() {
            return Ok(HealthWithStartTime::from_unhealthy(
                Unhealthy::new(e.to_string()),
                self.start_time,
            ));
        }

        Ok(HealthWithStartTime::from_healthy(
            Healthy::new(),
            self.start_time,
        ))
    }
}

#[cfg(test)]
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::health::health_checker::Health;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::DynamicObject;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    const TEST_VERSION: &str = "0.1.1";
    type MockResponse = Result<Option<Arc<DynamicObject>>, Arc<K8sError>>;
    struct MockResponses {
        crds: HashMap<&'static str, MockResponse>,
    }

    impl MockResponses {
        fn healthy() -> Self {
            let mut crds = HashMap::new();
            for &crd_name in EXPECTED_CRDS {
                let version_label = format!("flux2-{}", TEST_VERSION);
                crds.insert(
                    crd_name,
                    Ok(Some(Arc::new(DynamicObject {
                        types: Some(CRD_TYPEMETA.clone()),
                        metadata: ObjectMeta {
                            labels: Some(
                                vec![("helm.sh/chart".to_string(), version_label.clone())]
                                    .into_iter()
                                    .collect(),
                            ),
                            ..Default::default()
                        },
                        data: json!({}),
                    }))),
                );
            }
            Self { crds }
        }
    }

    fn setup_mock(mock: &mut MockSyncK8sClient, responses: MockResponses) {
        mock.expect_get_dynamic_object()
            .returning(move |type_meta, name, _namespace| {
                if type_meta.kind == "CustomResourceDefinition" {
                    let response = responses
                        .crds
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| panic!("Mock not configured for CRD: {}", name));

                    return response.map_err(|arc_error| match &*arc_error {
                        K8sError::GetDynamic(msg) => K8sError::GetDynamic(msg.clone()),
                        _ => panic!("Mock encountered an unhandled K8sError variant for cloning."),
                    });
                }
                panic!("Unexpected call to get_dynamic_object for: {:?}", type_meta);
            });
    }

    #[test]
    fn test_flux_crds_health() {
        type TestCase = (
            &'static str,
            Result<Health, HealthCheckerError>,
            MockResponses,
        );

        let test_cases: Vec<TestCase> = vec![
            (
                "CRDs are healthy when all are found with the correct version",
                Ok(Healthy::new().into()),
                MockResponses::healthy(),
            ),
            (
                "Fails when a CRD is not found",
                Ok(Unhealthy::new(
                    "Flux CRD not found: buckets.source.toolkit.fluxcd.io".to_string(),
                )
                    .into()),
                {
                    let mut responses = MockResponses::healthy();
                    responses
                        .crds
                        .insert("buckets.source.toolkit.fluxcd.io", Ok(None));
                    responses
                },
            ),
            (
                "Fails when a CRD has the wrong version",
                Ok(Unhealthy::new("Incorrect version for CRD 'gitrepositories.source.toolkit.fluxcd.io'. Expected: 0.1.1, Found: 0.1.2".to_string()).into()),
                {
                    let mut responses = MockResponses::healthy();
                    let version_label = "0.1.2".to_string();
                    responses.crds.insert("gitrepositories.source.toolkit.fluxcd.io", Ok(Some(Arc::new(
                        DynamicObject {
                            types: Some(CRD_TYPEMETA.clone()),
                            metadata: ObjectMeta {
                                labels: Some(vec![("helm.sh/chart".to_string(), version_label)].into_iter().collect()),
                                ..Default::default()
                            },
                            data: json!({})
                        }
                    ))));
                    responses
                }
            ),
            (
                "Fails when a CRD is missing the version label",
                Ok(Unhealthy::new("CRD 'helmcharts.source.toolkit.fluxcd.io' has no labels".to_string()).into()),
                {
                    let mut responses = MockResponses::healthy();
                    responses.crds.insert("helmcharts.source.toolkit.fluxcd.io", Ok(Some(Arc::new(
                        DynamicObject {
                            types: Some(CRD_TYPEMETA.clone()),
                            metadata: ObjectMeta { labels: None, ..Default::default() },
                            data: json!({})
                        }
                    ))));
                    responses
                }
            ),
            (
                "Fails when the k8s client returns an error",
                Ok(Unhealthy::new("Error checking CRD helmreleases.helm.toolkit.fluxcd.io: while getting dynamic resource: K8s API Error".to_string()).into()),
                {
                    let mut responses = MockResponses::healthy();
                    responses.crds.insert(
                        "helmreleases.helm.toolkit.fluxcd.io",
                        Err(Arc::new(K8sError::GetDynamic("K8s API Error".to_string()))),
                    );
                    responses
                }
            ),
        ];

        for (name, expected, responses) in test_cases {
            println!("Running test case: {}", name);
            let mut mock_client = MockSyncK8sClient::new();
            setup_mock(&mut mock_client, responses);

            let start_time = StartTime::now();
            let checker = FluxCrdsHealthChecker::new(
                Arc::new(mock_client),
                TypeMeta::default(),
                TEST_VERSION.to_string(),
                start_time,
            );
            let result = checker.check_health();

            match expected {
                Ok(expected_health) => {
                    let result_health = result
                        .unwrap_or_else(|err| panic!("Unexpected error '{err}' in test '{name}'"));
                    assert_eq!(
                        result_health,
                        HealthWithStartTime::new(expected_health, start_time),
                        "Failed test case: {}",
                        name
                    );
                }
                Err(expected_err) => {
                    let result_err = result.unwrap_err();
                    assert_eq!(
                        result_err.to_string(),
                        expected_err.to_string(),
                        "Failed test case: {}",
                        name
                    );
                }
            }
        }
    }
}
