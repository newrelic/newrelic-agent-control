use super::utils;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils as client_utils;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use std::sync::Arc;

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
            .filter(utils::flux_release_filter(self.release_name.clone()));

        let health =
            utils::check_health_for_items(target_stateful_sets, Self::stateful_set_health)?;

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
            .ok_or(utils::missing_field_error(sts, &name, ".spec"))?;
        let status = sts
            .status
            .as_ref()
            .ok_or_else(|| utils::missing_field_error(sts, &name, ".status"))?;

        let partition = Self::partition(spec).unwrap_or(0);
        let replicas = spec.replicas.unwrap_or(1);

        let expected_replicas = replicas - partition;

        if status.observed_generation.is_none() {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                "StatefulSet `{name}` is so new that it has no `observed_generation` status yet"
            ),
            )
            .into());
        };

        if status.observed_generation != sts.metadata.generation {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                    "StatefulSet `{name}` not ready: observed_generation not matching generation"
                ),
            )
            .into());
        }

        let updated_replicas = status
            .updated_replicas
            .ok_or_else(|| utils::missing_field_error(sts, &name, ".status.updatedReplicas"))?;
        if updated_replicas < expected_replicas {
            return Ok(Unhealthy::new(String::default(),format!(
                        "StatefulSet `{}` not ready: updated_replicas `{}` fewer than expected_replicas `{}`",
                        name,
                        updated_replicas,
                        expected_replicas,
                    )).into());
        }

        let ready_replicas = status
            .ready_replicas
            .ok_or_else(|| utils::missing_field_error(sts, &name, ".status.readyReplicas"))?;
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

        // TODO: should we fail if `status.current_revision` and/or `status.update_revision` are None?
        if partition == 0 && status.current_revision != status.update_revision {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                    "StatefulSet `{name}` not ready: current_revision not matching update_revision",
                ),
            )
            .into());
        }

        Ok(Healthy::new(String::default()).into())
    }

    /// Gets the partition from the stateful_set spec.
    fn partition(spec: &StatefulSetSpec) -> Option<i32> {
        spec.update_strategy
            .as_ref()
            .and_then(|update_strategy| update_strategy.rolling_update.as_ref())
            .and_then(|rolling_update| rolling_update.partition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_agent::health::health_checker::Healthy;
    use crate::{
        k8s::client::MockSyncK8sClient, sub_agent::health::k8s::health_checker::LABEL_RELEASE_FLUX,
    };
    use assert_matches::assert_matches;
    use k8s_openapi::api::apps::v1::{
        RollingUpdateStatefulSetStrategy, StatefulSetStatus, StatefulSetUpdateStrategy,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    /// Returns a [ObjectMeta] valid for for health-check
    fn stateful_set_meta(name: &str, generation: Option<i64>) -> ObjectMeta {
        ObjectMeta {
            name: Some(name.into()),
            generation,
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
    fn stateful_set_spec(partition: i32, replicas: i32) -> StatefulSetSpec {
        StatefulSetSpec {
            update_strategy: Some(StatefulSetUpdateStrategy {
                rolling_update: Some(RollingUpdateStatefulSetStrategy {
                    partition: Some(partition),
                    ..Default::default()
                }),
                ..Default::default()
            }),
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
                name: "Observed generation and matching generation don't match",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", Some(42)),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(stateful_set_status()),
                },
                expected: Unhealthy::new(String::default(),"StatefulSet `name` not ready: observed_generation not matching generation".into()).into()
            },
            TestCase {
                name: "Updated replicas fewer than expected",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(stateful_set_spec(3, 5)),
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(1),
                        ..stateful_set_status()
                    }),
                },
                expected: Unhealthy::new(String::default(),"StatefulSet `name` not ready: updated_replicas `1` fewer than expected_replicas `2`".into()).into(),
            },
            TestCase {
                name: "Not ready and ready replicas not matching",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(stateful_set_spec(3, 5)),
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(2),
                        ready_replicas: Some(1),
                        ..stateful_set_status()
                    }),
                },
                expected: Unhealthy::new(String::default(),"StatefulSet `name` not ready: replicas `5` different from ready_replicas `1`".into()).into(),
            },
            TestCase {
                name: "Current and update revision not matching when partition is 0",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(StatefulSetSpec::default()), // partition defaults to 0 when not defined
                    status: Some(StatefulSetStatus {
                        current_revision: Some("r1".to_string()),
                        update_revision: Some("r2".to_string()),
                        ..stateful_set_status()
                    }),
                },
                expected: Unhealthy::new(String::default(),"StatefulSet `name` not ready: current_revision not matching update_revision".into()).into(),
            },
            TestCase {
                name: "Healthy with not matching current and update revision",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(stateful_set_spec(3, 5)),
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(2),
                        ready_replicas: Some(5),
                        current_revision: Some("r1".to_string()),
                        update_revision: Some("r2".to_string()),
                        ..stateful_set_status()
                    }),
                },
                expected: Healthy::default().into(),
            },
            TestCase {
                name: "Healthy when partition is 0",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(StatefulSetSpec::default()), // partition and replicas default to 0 and 1
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(1),
                        ready_replicas: Some(1),
                        ..stateful_set_status()
                    }),
                },
                expected: Healthy::default().into(),
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
                    metadata: stateful_set_meta("name", None),
                    spec: None,
                    status: Some(stateful_set_status()),
                },
                err_check_fn: TestCase::assert_missing_field,
            },
            TestCase {
                name: "Invalid object, no status",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: Some(StatefulSetSpec::default()),
                    status: None,
                },
                err_check_fn: TestCase::assert_missing_field,
            },
            TestCase {
                name: "Invalid object, no .status.updatedReplicas",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(StatefulSetStatus {
                        updated_replicas: None,
                        ..stateful_set_status()
                    }),
                },
                err_check_fn: TestCase::assert_missing_field,
            },
            TestCase {
                name: "Invalid object, no .status.readyReplicas",
                sts: StatefulSet {
                    metadata: stateful_set_meta("name", None),
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
                ..stateful_set_meta("name", stateful_set_status().observed_generation)
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
