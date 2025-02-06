use super::utils;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils as client_utils;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetStatus};
use std::sync::Arc;

pub struct K8sHealthDaemonSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    start_time: StartTime,
}

impl HealthChecker for K8sHealthDaemonSet {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let daemon_sets = self.k8s_client.list_daemon_set();

        let target_daemon_sets = daemon_sets
            .into_iter()
            .filter(utils::flux_release_filter(self.release_name.clone()));

        let health = utils::check_health_for_items(
            target_daemon_sets,
            Self::check_health_single_daemon_set,
        )?;
        Ok(HealthWithStartTime::new(health, self.start_time))
    }
}

impl K8sHealthDaemonSet {
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

    // A DS is unHealthy if the number of ready replicas is < expected replicas ignoring rolling_update.max_unavailable
    // or if number_unavailable>0
    // We decided to ignore max_unavailable and therefore consider unhealthy a DaemonSet during a rolling update
    // Moreover, following the APM approach we are not considering updatedNumberScheduled.
    // I.e. we are reporting healthy also whenever there is an instance running an old version.
    pub fn check_health_single_daemon_set(ds: &DaemonSet) -> Result<Health, HealthCheckerError> {
        let name = client_utils::get_metadata_name(ds)?;
        let status = Self::get_daemon_set_status(name.as_str(), ds)?;
        if status.number_ready < status.desired_number_scheduled {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                    "DaemonSet '{}': The number of pods ready is less that the desired: {} < {}",
                    name, status.number_ready, status.desired_number_scheduled
                ),
            )
            .into());
        }

        if status
            .number_unavailable
            .is_some_and(|number_unavailable| number_unavailable > 0)
        {
            return Ok(Unhealthy::new(
                String::default(),
                format!(
                    "DaemonSet '{}': The are {} unavailable pods",
                    name,
                    status.number_unavailable.unwrap_or_default()
                ),
            )
            .into());
        }

        Ok(Healthy::new(String::default()).into())
    }

    fn get_daemon_set_status(
        name: &str,
        daemon_set: &DaemonSet,
    ) -> Result<DaemonSetStatus, HealthCheckerError> {
        daemon_set
            .status
            .clone()
            .ok_or_else(|| utils::missing_field_error(daemon_set, name, ".status"))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        k8s::client::MockSyncK8sClient,
        sub_agent::health::{
            health_checker::{Healthy, Unhealthy},
            k8s::health_checker::LABEL_RELEASE_FLUX,
        },
    };
    use k8s_openapi::Resource as _; // Needed to access resource's KIND. e.g.: Deployment::KIND
    use k8s_openapi::{
        api::apps::v1::{DaemonSetSpec, DaemonSetStatus},
        apimachinery::pkg::apis::meta::v1::ObjectMeta,
    };

    const TEST_DAEMON_SET_NAME: &str = "test";

    #[test]
    fn test_daemon_set_spec_errors() {
        struct TestCase {
            name: &'static str,
            ds: DaemonSet,
            expected: HealthCheckerError,
        }

        impl TestCase {
            fn run(self) {
                let err_result = K8sHealthDaemonSet::check_health_single_daemon_set(&self.ds)
                    .inspect(|ok| {
                        panic!(
                            "Test Case '{}' is returning a Health Result: {:?}",
                            self.name, ok
                        );
                    })
                    .unwrap_err();

                assert_eq!(
                    err_result.to_string(),
                    self.expected.to_string(),
                    "{}",
                    self.name
                );
            }
        }

        let test_cases = vec![
            TestCase {
                name: "ds without metadata name",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: None,
                        ..Default::default()
                    },
                    spec: None,
                    status: None,
                },
                expected: HealthCheckerError::K8sError(crate::k8s::error::K8sError::MissingName(
                    DaemonSet::KIND.to_string(),
                )),
            },
            TestCase {
                name: "ds without status",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: None,
                    status: None,
                },
                expected: test_util_missing_field(".status"),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_daemon_set_health() {
        struct TestCase {
            name: &'static str,
            ds: DaemonSet,
            expected: Health,
        }

        impl TestCase {
            fn run(self) {
                let health_run: Result<Health, HealthCheckerError> =
                    K8sHealthDaemonSet::check_health_single_daemon_set(&self.ds);
                let health_result = health_run.unwrap_or_else(|err| {
                    panic!(
                        "Test case '{}' is not returning a Health Result: {}",
                        self.name, err
                    )
                });
                assert_eq!(health_result, self.expected, "{}", self.name);
            }
        }

        let test_cases = vec![
            TestCase {
                name: "ds with not enough ready pods",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        desired_number_scheduled: 3,
                        number_ready: 2,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy {
                    last_error: String::from(
                        "DaemonSet 'test': The number of pods ready is less that the desired: 2 < 3",
                    ),
                    ..Default::default()
                }
                .into(),
            },
            TestCase {
                name: "ds with unavailable pods",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        number_unavailable: Some(5),
                        ..Default::default()
                    }),
                },
                expected: Unhealthy {
                    last_error: String::from(
                        "DaemonSet 'test': The are 5 unavailable pods",
                    ),
                    ..Default::default()
                }
                .into(),
            },
            TestCase {
                name: "everything is good",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        desired_number_scheduled: 3,
                        number_ready: 3,
                        number_unavailable: Some(0),
                        ..Default::default()
                    }),
                },
                expected: Healthy::default().into(),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_check_health() {
        let mut k8s_client = MockSyncK8sClient::new();
        let release_name = "flux-release";

        let healthy_matching = DaemonSet {
            metadata: ObjectMeta {
                name: Some("healthy-daemon-set".to_string()),
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..Default::default()
            },
            spec: Some(DaemonSetSpec {
                ..Default::default()
            }),
            status: Some(DaemonSetStatus {
                ..Default::default()
            }),
        };

        let unhealthy_matching = DaemonSet {
            metadata: ObjectMeta {
                name: Some("unhealthy-daemon-set".to_string()),
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..Default::default()
            },
            spec: Some(DaemonSetSpec {
                ..Default::default()
            }),
            status: Some(DaemonSetStatus {
                desired_number_scheduled: 5,
                number_ready: 2,
                ..Default::default()
            }),
        };

        k8s_client
            .expect_list_daemon_set()
            .times(1)
            .returning(move || {
                vec![
                    Arc::new(healthy_matching.clone()),
                    Arc::new(unhealthy_matching.clone()),
                ]
            });

        let start_time = StartTime::now();

        let health_checker =
            K8sHealthDaemonSet::new(Arc::new(k8s_client), release_name.to_string(), start_time);
        let health = health_checker.check_health().unwrap();

        assert_eq!(
            health,
            HealthWithStartTime::from_unhealthy(
                Unhealthy::new(String::default(), "DaemonSet 'unhealthy-daemon-set': The number of pods ready is less that the desired: 2 < 5".into()),
                start_time
            )
        );
    }

    fn test_util_get_common_metadata() -> ObjectMeta {
        ObjectMeta {
            name: Some(TEST_DAEMON_SET_NAME.to_string()),
            ..Default::default()
        }
    }

    fn test_util_missing_field(field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField {
            kind: DaemonSet::KIND.to_string(),
            name: TEST_DAEMON_SET_NAME.to_string(),
            field: field.to_string(),
        }
    }
}
