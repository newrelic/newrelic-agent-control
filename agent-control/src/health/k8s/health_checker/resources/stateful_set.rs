use crate::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils as client_utils;
use k8s_openapi::api::apps::v1::StatefulSet;
use std::sync::Arc;

use super::{check_health_for_items, flux_release_filter, missing_field_error};

/// Represents a health checker for the StatefulSets or a release.
pub struct K8sHealthStatefulSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    start_time: StartTime,
}

impl HealthChecker for K8sHealthStatefulSet {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let stateful_sets = self.k8s_client.list_stateful_set();

        let target_stateful_sets = stateful_sets
            .into_iter()
            .filter(flux_release_filter(self.release_name.clone()));

        let health = check_health_for_items(target_stateful_sets, Self::stateful_set_health)?;

        Ok(HealthWithStartTime::new(health, self.start_time))
    }
}

impl K8sHealthStatefulSet {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        release_name: String,
        start_time: StartTime,
    ) -> Self {
        Self {
            k8s_client,
            release_name,
            start_time,
        }
    }

    /// Returns the health for a single stateful_set.
    fn stateful_set_health(sts: &StatefulSet) -> Result<Health, HealthCheckerError> {
        let name = client_utils::get_metadata_name(sts)?;
        let spec = sts
            .spec
            .as_ref()
            .ok_or(missing_field_error(sts, &name, ".spec"))?;
        let status = sts
            .status
            .as_ref()
            .ok_or_else(|| missing_field_error(sts, &name, ".status"))?;

        let replicas = spec.replicas.unwrap_or(1);

        let ready_replicas = status
            .ready_replicas
            .ok_or_else(|| missing_field_error(sts, &name, ".status.readyReplicas"))?;

        if replicas != ready_replicas {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                    "StatefulSet `{}` not ready: replicas `{}` different from ready_replicas `{}`",
                    name, replicas, ready_replicas,
                ),
            )
            .into());
        }

        Ok(Healthy::new(String::default()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::health_checker::Healthy;
    use crate::{health::k8s::health_checker::LABEL_RELEASE_FLUX, k8s::client::MockSyncK8sClient};
    use assert_matches::assert_matches;
    use k8s_openapi::api::apps::v1::{StatefulSetSpec, StatefulSetStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    /// Returns a [ObjectMeta] valid for for health-check
    fn stateful_set_meta(name: &str) -> ObjectMeta {
        ObjectMeta {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Returns a [StatefulSetStatus] valid for health-check
    fn stateful_set_status() -> StatefulSetStatus {
        StatefulSetStatus {
            updated_replicas: Some(1),
            ready_replicas: Some(1),
            current_revision: Some("rev".into()),
            update_revision: Some("rev".into()),
            observed_generation: Some(1),
            ..Default::default()
        }
    }

    /// Returns a [StatefulSetSpec] given the partition and replicas.
    fn stateful_set_spec(replicas: i32) -> StatefulSetSpec {
        StatefulSetSpec {
            replicas: Some(replicas),
            ..Default::default()
        }
    }

    #[test]
    fn test_stateful_set_health() {
        struct TestCase {
            name: &'static str,
            sts: StatefulSet,
            expected: Health,
        }
        impl TestCase {
            fn run(self) {
                let (name, sts, expected) = (self.name, self.sts, self.expected);
                let result = K8sHealthStatefulSet::stateful_set_health(&sts)
                    .inspect_err(|err| {
                        panic!("Unexpected error getting health: {} - {}", err, name);
                    })
                    .unwrap();
                assert_eq!(expected, result, "{}", name);
            }
        }

        let test_cases = [
            TestCase {
                name: "Ready replicas matches expected replicas",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name"),
                    spec: Some(stateful_set_spec(5)),
                    status: Some(StatefulSetStatus {
                        ready_replicas: Some(5),
                        ..Default::default()
                    }),
                },
                expected: Healthy::default().into(),
            },
            TestCase {
                name: "Ready replicas is lower than expected replicas",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name"),
                    spec: Some(stateful_set_spec(5)),
                    status: Some(StatefulSetStatus {
                        ready_replicas: Some(4),
                        ..Default::default()
                    }),
                },
                expected: Unhealthy::new(
                    String::default(),
                    "StatefulSet `name` not ready: replicas `5` different from ready_replicas `4`"
                        .into(),
                )
                .into(),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_stateful_set_health_errors() {
        struct TestCase {
            name: &'static str,
            sts: StatefulSet,
            err_check_fn: fn(HealthCheckerError, &str),
        }

        impl TestCase {
            fn run(&self) {
                let err = K8sHealthStatefulSet::stateful_set_health(&self.sts.clone())
                    .inspect(|result| {
                        panic!("Expected error, got {:?} for test - {}", result, self.name)
                    })
                    .unwrap_err();
                (self.err_check_fn)(err, self.name)
            }
            fn assert_missing_field(err: HealthCheckerError, name: &str) {
                assert_matches!(
                    err,
                    HealthCheckerError::MissingK8sObjectField {
                        kind: _,
                        name: _,
                        field: _
                    },
                    "{}",
                    name
                );
            }
            fn assert_missing_metadata_name(err: HealthCheckerError, name: &str) {
                assert_matches!(err, HealthCheckerError::K8sError(_), "{}", name);
            }
        }

        let test_cases = [
            TestCase {
                name: "Invalid object, no name",
                sts: StatefulSet::default(),
                err_check_fn: TestCase::assert_missing_metadata_name,
            },
            TestCase {
                name: "Invalid object, no spec",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name"),
                    spec: None,
                    status: Some(stateful_set_status()),
                },
                err_check_fn: TestCase::assert_missing_field,
            },
            TestCase {
                name: "Invalid object, no status",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name"),
                    spec: Some(StatefulSetSpec::default()),
                    status: None,
                },
                err_check_fn: TestCase::assert_missing_field,
            },
            TestCase {
                name: "Invalid object, no .status.readyReplicas",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name"),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(StatefulSetStatus {
                        ready_replicas: None,
                        ..stateful_set_status()
                    }),
                },
                err_check_fn: TestCase::assert_missing_field,
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_check_health() {
        let mut k8s_client = MockSyncK8sClient::new();
        let release_name = "flux-release";

        let healthy_matching = StatefulSet {
            metadata: ObjectMeta {
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..stateful_set_meta("name")
            },
            spec: Some(StatefulSetSpec::default()),
            status: Some(StatefulSetStatus {
                updated_replicas: Some(1),
                ready_replicas: Some(1),
                ..stateful_set_status()
            }),
        };

        let with_err_not_matching = StatefulSet {
            metadata: ObjectMeta {
                labels: Some(
                    [(
                        LABEL_RELEASE_FLUX.to_string(),
                        "another-release".to_string(),
                    )]
                    .into(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };

        k8s_client
            .expect_list_stateful_set()
            .times(1)
            .returning(move || {
                vec![
                    Arc::new(with_err_not_matching.clone()),
                    Arc::new(healthy_matching.clone()),
                ]
            });

        let start_time = StartTime::now();

        let health_checker =
            K8sHealthStatefulSet::new(Arc::new(k8s_client), release_name.to_string(), start_time);
        let result = health_checker.check_health().unwrap();
        assert_eq!(
            result,
            HealthWithStartTime::from_healthy(Healthy::default(), start_time)
        );
    }
}
