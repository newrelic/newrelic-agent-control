use crate::checkers::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::checkers::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::k8s::client::{K8sClient, SyncK8sClient};
use crate::k8s::utils as client_utils;
use k8s_openapi::api::apps::v1::StatefulSet;
use std::sync::Arc;

use super::{
    ResourceFilter, check_health_for_items, flux_release_filter, missing_field_error, name_filter,
};

/// Represents a health checker for a StatefulSet resource.
#[derive(Debug)]
pub struct K8sHealthStatefulSet<C: K8sClient = SyncK8sClient> {
    k8s_client: Arc<C>,
    filter: ResourceFilter,
    start_time: StartTime,
    namespace: String,
}

impl<C: K8sClient> HealthChecker for K8sHealthStatefulSet<C> {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let stateful_sets = self.k8s_client.list_stateful_set(&self.namespace)?;

        let health = match &self.filter {
            ResourceFilter::ByName(name) => check_health_for_items(
                stateful_sets.into_iter().filter(name_filter(name.clone())),
                K8sHealthStatefulSet::stateful_set_health,
            )?,
            ResourceFilter::ByFluxLabel(release) => check_health_for_items(
                stateful_sets
                    .into_iter()
                    .filter(flux_release_filter(release.clone())),
                K8sHealthStatefulSet::stateful_set_health,
            )?,
        };

        Ok(HealthWithStartTime::new(health, self.start_time))
    }
}

impl K8sHealthStatefulSet {
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
            return Ok(Unhealthy::new(format!(
                "StatefulSet `{name}` not ready: replicas `{replicas}` different from ready_replicas `{ready_replicas}`",
            ))
            .into());
        }

        Ok(Healthy::new().into())
    }
}

impl<C: K8sClient> K8sHealthStatefulSet<C> {
    pub fn new(
        k8s_client: Arc<C>,
        filter: ResourceFilter,
        start_time: StartTime,
        namespace: String,
    ) -> Self {
        Self {
            k8s_client,
            filter,
            start_time,
            namespace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkers::health::health_checker::Healthy;
    use crate::checkers::health::k8s::health_checker::resources::daemon_set::tests::TEST_NAMESPACE;
    use crate::k8s::client::tests::MockK8sClient;
    use assert_matches::assert_matches;
    use k8s_openapi::api::apps::v1::{StatefulSetSpec, StatefulSetStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    impl<C: K8sClient> K8sHealthStatefulSet<C> {
        pub(crate) fn namespace(&self) -> &str {
            &self.namespace
        }
    }

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
                        panic!("Unexpected error getting health: {err} - {name}");
                    })
                    .unwrap();
                assert_eq!(expected, result, "{name}");
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
                expected: Healthy::new().into(),
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
                assert_matches!(err, HealthCheckerError::K8sError(err) => assert_eq!(
                err.to_string(),
                "StatefulSet does not have .metadata.name"
            ), "{}", name);
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
        let mut k8s_client = MockK8sClient::new();
        let name = "target-stateful-set";

        // Matches by name — healthy.
        let matching_healthy = StatefulSet {
            metadata: stateful_set_meta(name),
            spec: Some(StatefulSetSpec::default()),
            status: Some(StatefulSetStatus {
                ready_replicas: Some(1),
                ..stateful_set_status()
            }),
        };

        // Does not match by name — would error if checked, but must be skipped.
        let non_matching = StatefulSet {
            metadata: stateful_set_meta("other-stateful-set"),
            ..Default::default()
        };

        k8s_client
            .expect_list_stateful_set()
            .times(1)
            .returning(move |_| {
                Ok(vec![
                    Arc::new(non_matching.clone()),
                    Arc::new(matching_healthy.clone()),
                ])
            });

        let start_time = StartTime::now();
        let health_checker = K8sHealthStatefulSet::new(
            Arc::new(k8s_client),
            ResourceFilter::ByName(name.to_string()),
            start_time,
            TEST_NAMESPACE.to_string(),
        );
        let result = health_checker.check_health().unwrap();
        assert_eq!(
            result,
            HealthWithStartTime::from_healthy(Healthy::new(), start_time)
        );
    }

    #[test]
    fn test_check_health_for_helm_release() {
        use crate::checkers::health::k8s::health_checker::LABEL_RELEASE_FLUX;
        let mut k8s_client = MockK8sClient::new();
        let release_name = "flux-release";

        // Matches by Flux label — healthy.
        let matching_healthy = StatefulSet {
            metadata: ObjectMeta {
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..stateful_set_meta("chart-stateful-set")
            },
            spec: Some(StatefulSetSpec::default()),
            status: Some(StatefulSetStatus {
                ready_replicas: Some(1),
                ..stateful_set_status()
            }),
        };

        // Does not carry the Flux label — would error if checked, but must be skipped.
        let non_matching = StatefulSet {
            metadata: stateful_set_meta("other-stateful-set"),
            ..Default::default()
        };

        k8s_client
            .expect_list_stateful_set()
            .times(1)
            .returning(move |_| {
                Ok(vec![
                    Arc::new(non_matching.clone()),
                    Arc::new(matching_healthy.clone()),
                ])
            });

        let start_time = StartTime::now();
        let health_checker = K8sHealthStatefulSet::new(
            Arc::new(k8s_client),
            ResourceFilter::ByFluxLabel(release_name.to_string()),
            start_time,
            TEST_NAMESPACE.to_string(),
        );
        let result = health_checker.check_health().unwrap();
        assert_eq!(
            result,
            HealthWithStartTime::from_healthy(Healthy::new(), start_time)
        );
    }
}
