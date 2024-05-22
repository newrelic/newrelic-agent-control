use crate::k8s::client::contains_label_with_value;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use crate::sub_agent::health::k8s::health_checker::LABEL_RELEASE_FLUX;
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use std::sync::Arc;

/// Represents a health checker for the StatefulSets or a release.
pub struct K8sHealthStatefulSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    // TODO Waiting on https://github.com/kube-rs/kube/pull/1482, Hardcoding label for now
    // label_selector: String,
}

impl HealthChecker for K8sHealthStatefulSet {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        // TODO: Replace with reflector when ready
        let stateful_sets = self.k8s_client.list_stateful_set().map_err(|err| {
            HealthCheckerError::Generic(format!(
                "Error fetching StatefulSets '{}': {}",
                &self.release_name, err
            ))
        })?;

        let release_name = self.release_name.as_str();
        let matching_stateful_sets = stateful_sets.into_iter().filter(|ss| {
            contains_label_with_value(&ss.metadata.labels, LABEL_RELEASE_FLUX, release_name)
        });

        for ss in matching_stateful_sets {
            let ss_health = K8sHealthStatefulSet::stateful_set_health(ss)?;
            if !ss_health.is_healthy() {
                return Ok(ss_health);
            }
        }

        Ok(Healthy::default().into())
    }
}

impl K8sHealthStatefulSet {
    pub fn new(k8s_client: Arc<SyncK8sClient>, release_name: String) -> Self {
        Self {
            k8s_client,
            release_name,
        }
    }

    /// Returns the health for a single stateful_set.
    fn stateful_set_health(ss: StatefulSet) -> Result<Health, HealthCheckerError> {
        let name = ss
            .metadata
            .name
            .ok_or_else(|| HealthCheckerError::Generic("StatefulSets without Name".to_string()))?;
        let spec = ss.spec.ok_or(Self::missing_field_error(&name, "Spec"))?;
        let status = ss
            .status
            .ok_or_else(|| Self::missing_field_error(&name, "Status"))?;

        let partition = Self::partition(&spec).unwrap_or(0);
        let replicas = spec.replicas.unwrap_or(1);

        let expected_replicas = replicas - partition;

        // TODO: should we fail if `status.observed_generation` or `metadata.observed_generation are none?
        if status.observed_generation != ss.metadata.generation {
            return Ok(Health::unhealthy_with_last_error(format!(
                "StatefulSets `{}` not ready: observed_generation not matching generation",
                name
            )));
        }

        let updated_replicas = status
            .updated_replicas
            .ok_or_else(|| Self::missing_field_error(&name, "Status.UpdatedReplicas"))?;
        if updated_replicas < expected_replicas {
            return Ok(Health::unhealthy_with_last_error(format!(
                        "StatefulSets `{}` not ready: updated_replicas `{}` fewer than expected_replicas `{}`",
                        name,
                        updated_replicas,
                        expected_replicas,
                    )));
        }

        let ready_replicas = status
            .ready_replicas
            .ok_or_else(|| Self::missing_field_error(&name, "Status.ReadyReplicas"))?;
        if replicas != ready_replicas {
            return Ok(Health::unhealthy_with_last_error(format!(
                "StatefulSets `{}` not ready: replicas `{}` different from ready_replicas `{}`",
                name, replicas, ready_replicas,
            )));
        }

        // TODO: should we fail if `status.current_revision` and/or `status.update_revision` are None?
        if partition == 0 && status.current_revision != status.update_revision {
            return Ok(Health::unhealthy_with_last_error(format!(
                "StatefulSets `{}` not ready: current_revision not matching update_revision",
                name
            )));
        }

        Ok(Healthy::default().into())
    }

    /// Helper to return an error when an expected field in the StatefulSet object is missing.
    fn missing_field_error(name: &str, field: &str) -> HealthCheckerError {
        HealthCheckerError::Generic(format!("StatefulSets `{}` without {}", name, field))
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
mod test {
    use super::*;
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
            name: String,
            ss: StatefulSet,
            expected: Health,
        }
        impl TestCase {
            fn run(self) {
                let (name, ss, expected) = (self.name, self.ss, self.expected);
                let result = K8sHealthStatefulSet::stateful_set_health(ss)
                    .inspect_err(|err| {
                        panic!("Unexpected error getting health: {} - {}", err, name);
                    })
                    .unwrap();
                assert_eq!(expected, result, "{}", name);
            }
        }

        let test_cases = [
            TestCase {
                name: "Observed generation and matching generation don't match".to_string(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", Some(42)),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(stateful_set_status()),
                },
                expected: Health::unhealthy_with_last_error("StatefulSets `name` not ready: observed_generation not matching generation".into())
            },
            TestCase {
                name: "Updated replicas fewer than expected".to_string(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(stateful_set_spec(3, 5)),
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(1),
                        ..stateful_set_status()
                    }),
                },
                expected: Health::unhealthy_with_last_error("StatefulSets `name` not ready: updated_replicas `1` fewer than expected_replicas `2`".into()),
            },
            TestCase {
                name: "Not ready and ready replicas not matching".to_string(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(stateful_set_spec(3, 5)),
                    status: Some(StatefulSetStatus {
                        updated_replicas: Some(2),
                        ready_replicas: Some(1),
                        ..stateful_set_status()
                    }),
                },
                expected: Health::unhealthy_with_last_error("StatefulSets `name` not ready: replicas `5` different from ready_replicas `1`".into()),
            },
            TestCase {
                name: "Current and update revision not matching when partition is 0".to_string(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", stateful_set_status().observed_generation),
                    spec: Some(StatefulSetSpec::default()), // partition defaults to 0 when not defined
                    status: Some(StatefulSetStatus {
                        current_revision: Some("r1".to_string()),
                        update_revision: Some("r2".to_string()),
                        ..stateful_set_status()
                    }),
                },
                expected: Health::unhealthy_with_last_error("StatefulSets `name` not ready: current_revision not matching update_revision".into()),
            },
            TestCase {
                name: "Healthy with not matching current and update revision".to_string(),
                ss: StatefulSet {
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
                name: "Healthy when partition is 0".to_string(),
                ss: StatefulSet {
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
            name: String,
            ss: StatefulSet,
            expected_err: HealthCheckerError,
        }

        impl TestCase {
            fn run(self) {
                let (name, ss, expected_err) = (self.name, self.ss, self.expected_err);
                let err = K8sHealthStatefulSet::stateful_set_health(ss)
                    .inspect(|result| {
                        panic!("Expected error, got {:?} for test - {}", result, name)
                    })
                    .unwrap_err();
                assert_eq!(err.to_string(), expected_err.to_string());
            }
        }

        let test_cases = [
            TestCase {
                name: "Invalid object, no name".into(),
                ss: StatefulSet::default(),
                expected_err: HealthCheckerError::Generic("StatefulSets without Name".into()),
            },
            TestCase {
                name: "Invalid object, no spec".into(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: None,
                    status: Some(stateful_set_status()),
                },
                expected_err: HealthCheckerError::Generic(
                    "StatefulSets `name` without Spec".into(),
                ),
            },
            TestCase {
                name: "Invalid object, no status".into(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: Some(StatefulSetSpec::default()),
                    status: None,
                },
                expected_err: HealthCheckerError::Generic(
                    "StatefulSets `name` without Status".into(),
                ),
            },
            TestCase {
                name: "Invalid object, no Status.UpdatedReplicas".into(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(StatefulSetStatus {
                        updated_replicas: None,
                        ..stateful_set_status()
                    }),
                },
                expected_err: HealthCheckerError::Generic(
                    "StatefulSets `name` without Status.UpdatedReplicas".into(),
                ),
            },
            TestCase {
                name: "Invalid object, no Status.ReadyReplicas".into(),
                ss: StatefulSet {
                    metadata: stateful_set_meta("name", None),
                    spec: Some(StatefulSetSpec::default()),
                    status: Some(StatefulSetStatus {
                        ready_replicas: None,
                        ..stateful_set_status()
                    }),
                },
                expected_err: HealthCheckerError::Generic(
                    "StatefulSets `name` without Status.ReadyReplicas".into(),
                ),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
