use k8s_openapi::api::apps::v1::DaemonSet;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::{DaemonSetUpdateStrategies, IntOrPercentage};
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use std::sync::Arc;

use super::health_checker::LABEL_RELEASE_FLUX;

pub struct K8sHealthDaemonSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    // TODO Waiting on https://github.com/kube-rs/kube/pull/1482, Hardcoding label for now
    // label_selector: String,
}

impl HealthChecker for K8sHealthDaemonSet {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let daemon_set_list = self.k8s_client.list_daemon_set().map_err(|e| {
            HealthCheckerError::new(format!(
                "Error fetching DaemonSet '{}': {}",
                &self.release_name, e
            ))
        })?;

        let filtered_list_daemon_set = daemon_set_list.into_iter().filter(|ds| {
            crate::k8s::utils::is_label_present(
                &ds.metadata.labels,
                LABEL_RELEASE_FLUX,
                self.release_name.as_str(),
            )
        });

        for ds in filtered_list_daemon_set {
            let health = K8sHealthDaemonSet::check_health_single_daemon_set(ds)?;
            if !health.is_healthy() {
                return Ok(health);
            }
        }

        Ok(Healthy::default().into())
    }
}

impl K8sHealthDaemonSet {
    pub fn new(k8s_client: Arc<SyncK8sClient>, release_name: String) -> Self {
        Self {
            k8s_client,
            release_name,
        }
    }

    pub fn check_health_single_daemon_set(ds: DaemonSet) -> Result<Health, HealthCheckerError> {
        let name = match ds.metadata.name {
            Some(s) => s,
            None => {
                return Err(HealthCheckerError::new(
                    "Daemonset has no .metadata.name".into(),
                ));
            }
        };

        let status = match ds.status {
            Some(daemon_set_status) => daemon_set_status,
            None => {
                return Ok(Unhealthy {
                    status: "DaemonSet unhealthy".to_string(),
                    last_error: format!("Daemonset '{name}' is so new that it has no status yet"),
                }
                .into())
            }
        };

        let update_strategy = ds
            .spec
            .ok_or(HealthCheckerError::new(format!(
                "Daemonset '{name}' has no spec"
            )))?
            .update_strategy
            .ok_or(HealthCheckerError::new(format!(
                "Daemonset '{name}' has no update strategy"
            )))?;

        let rolling_update = match DaemonSetUpdateStrategies::from(update_strategy.type_) {
            DaemonSetUpdateStrategies::Unknown(s) => {
                return Err(HealthCheckerError::new(format!(
                    "Daemonset '{name}' has an unknown Update Strategy Type: '{s}'"
                )));
            }
            // If the update strategy is not a rolling update, there will be nothing to wait for
            DaemonSetUpdateStrategies::OnDelete => {
                return Ok(Healthy {
                    status: format!(
                        "Daemonset '{name}' has on delete upgrade strategy. No health to check."
                    ),
                }
                .into())
            }
            DaemonSetUpdateStrategies::RollingUpdate => {
                update_strategy.rolling_update.ok_or_else(|| {
                    HealthCheckerError::new(format!(
                        "Daemonset '{name}' has rolling update strategy type and no struct"
                    ))
                })?
            }
        };

        if status.updated_number_scheduled.is_none() {
            return Ok(Unhealthy {
                status: "DaemonSet unhealthy".to_string(),
                last_error: format!("Daemonset '{name}' is so new that it has no `updated_number_scheduled` status yet"),
            }.into());
        }

        // Make sure all the updated pods have been scheduled
        if let Some(updated_number_scheduled) = status.updated_number_scheduled {
            if updated_number_scheduled != status.desired_number_scheduled {
                return Ok(Unhealthy {
                    status: "DaemonSet unhealthy".to_string(),
                    last_error: format!(
                        "Not all the pods of the DaemonSet '{name}' were able to schedule"
                    ),
                }
                .into());
            }
        }

        let max_unavailable = if let Some(max_unavailable) = rolling_update.max_unavailable {
            // `rolling_update.max_unavailable` can me an integer (number of pods) or a percent (percent of
            // desired pods). The integer path is simple, but if it is a percent we have to calculate the percent
            // against the number of desired pods to know how many unavailable pods should be the maximum.
            match IntOrPercentage::from(max_unavailable) {
                IntOrPercentage::Int(i) => i,
                IntOrPercentage::Percentage(percent) => {
                    (status.desired_number_scheduled as f32 * percent).ceil() as i32
                }
                IntOrPercentage::Unknown(err) => {
                    return Err(HealthCheckerError::new(format!(
                        "Daemonset '{name}' has an non-parsable Max Availability on Update Strategy: '{err}'"
                    )))
                }
            }
        } else {
            // If max unavailable is not set, the daemonset does not expect to have healthy pods.
            // Returning Healthiness as soon as possible.
            return Ok(Healthy {
                status: format!(
                    "DaemonSet '{name}' healthy: This daemonset does not expect to have healthy pods",
                ),
            }.into());
        };

        let expected_ready = status.desired_number_scheduled - max_unavailable;
        if status.number_ready < expected_ready {
            return Ok(Unhealthy {
                status: "DaemonSet unhealthy".to_string(),
                last_error: format!(
                    "Daemonset '{name}': The number of pods ready is less that the desired: {} < {}",
                    status.number_ready, expected_ready
                ),
            }.into());
        }

        Ok(Healthy {
            status: format!(
                "DaemonSet '{name}' healthy: Pods ready are equal or greater than desired: {} >= {}",
                status.number_ready, expected_ready
            ),
        }.into())
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use k8s_openapi::{
        api::apps::v1::{
            DaemonSetSpec, DaemonSetStatus, DaemonSetUpdateStrategy, RollingUpdateDaemonSet,
        },
        apimachinery::pkg::{apis::meta::v1::ObjectMeta, util::intstr::IntOrString},
    };

    #[derive(Debug)]
    struct TestCase {
        name: &'static str,
        ds: DaemonSet,
        expected: &'static str,
    }

    #[test]
    fn test_invalid_daemonset_specs() {
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                name: "ds without status",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: None,
                    status: None,
                },
                expected: "Daemonset 'test' is so new that it has no status yet",
            },
            TestCase {
                name: "ds without updated_number_scheduled",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy{
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet{
                                ..Default::default()
                            })}),
                            ..Default::default()
                        }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: None,
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' is so new that it has no `updated_number_scheduled` status yet",
            },
        ];

        for test_case in test_cases {
            let health_run: Result<Health, HealthCheckerError> =
                K8sHealthDaemonSet::check_health_single_daemon_set(test_case.ds);
            let health_result = health_run.unwrap_or_else(|err| {
                panic!(
                    "Test case '{}' is not returning a Health Result: {}",
                    test_case.name, err
                )
            });
            let last_error = health_result.last_error().unwrap_or_else(|| {
                panic!(
                    "Test case '{}' is not returning a last error",
                    test_case.name
                )
            });
            assert_eq!(last_error, test_case.expected);
        }
    }

    #[test]
    fn test_daemonset_spec_errors() {
        let test_cases: Vec<TestCase> = vec![
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
                expected: "Daemonset has no .metadata.name",
            },
            TestCase {
                name: "ds without spec",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: None,
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' has no spec",
            },
            TestCase {
                name: "ds without update strategy",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: None,
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' has no update strategy",
            },
            TestCase {
                name: "ds with unknown update strategy",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(String::from("Unknown-TEST")),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' has an unknown Update Strategy Type: 'Unknown-TEST'",
            },
            TestCase {
                name: "ds which update strategy is rolling but has no struct",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: None,
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' has rolling update strategy type and no struct",
            },
            TestCase {
                name: "ds which update strategy is rolling but has no struct",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: None,
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: "Daemonset 'test' has rolling update strategy type and no struct",
            },
            TestCase {
                name: "ds update strategy policy has non-parsable max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: Some(IntOrString::String(String::from("NaN"))),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(1),
                        desired_number_scheduled: 1,
                        ..Default::default()
                    }),
                },
                expected:
                    "Daemonset 'test' has an non-parsable Max Availability on Update Strategy: 'invalid digit found in string'",
            },
        ];

        for test_case in test_cases {
            let health_run = K8sHealthDaemonSet::check_health_single_daemon_set(test_case.ds);
            let err_result = match health_run {
                Ok(ok) => panic!(
                    "Test case '{}' is returning a Health Result: {:?}",
                    test_case.name, ok,
                ),
                Err(err) => err,
            };

            // HealthCheckerError can add a Prefix to the expected String. To not tight these tests to the implementation
            // of HealthCheckerError I am wrapping the expectation and converting it to string so they can be compared.
            let health_checker_wrapper = HealthCheckerError::new(String::from(test_case.expected));

            assert_eq!(err_result.to_string(), health_checker_wrapper.to_string());
        }
    }

    #[test]
    fn test_daemonset_on_delete_update_strategy() {
        let test_case = TestCase {
            name: "ds which update strategy is on delete is always healthy",
            ds: DaemonSet {
                metadata: ObjectMeta {
                    name: Some(String::from("test")),
                    ..Default::default()
                },
                spec: Some(DaemonSetSpec {
                    update_strategy: Some(DaemonSetUpdateStrategy {
                        type_: Some(DaemonSetUpdateStrategies::OnDelete.to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                status: Some(DaemonSetStatus {
                    ..Default::default()
                }),
            },
            expected: "Daemonset 'test' has on delete upgrade strategy. No health to check.",
        };

        let health_run = K8sHealthDaemonSet::check_health_single_daemon_set(test_case.ds);
        let health_result = health_run.unwrap_or_else(|err| {
            panic!(
                "Test case '{}' is not returning a Health Result: {}",
                test_case.name, err
            )
        });
        match health_result {
            Health::Unhealthy(unhealthy) => panic!(
                "Test case '{}' is not returning a healthy status: {:?}",
                test_case.name, unhealthy
            ),
            Health::Healthy(healthy) => assert_eq!(healthy.status(), test_case.expected),
        };
    }

    #[derive(Debug)]
    struct TestHealthCase {
        name: &'static str,
        ds: DaemonSet,
        expected: Health,
    }

    #[test]
    fn test_daemonset_healthiness() {
        let test_cases: Vec<TestHealthCase> = vec![
            TestHealthCase {
                name: "ds with no unschedulable pods",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(0),
                        desired_number_scheduled: 1,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy {
                    status: String::from("DaemonSet unhealthy"),
                    last_error: String::from(
                        "Not all the pods of the DaemonSet 'test' were able to schedule",
                    ),
                }.into(),
            },
            TestHealthCase {
                name: "ds without max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: None,
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(5),
                        desired_number_scheduled: 5,
                        ..Default::default()
                    }),
                },
                expected: Healthy {
                    status: String::from(
                        "DaemonSet 'test' healthy: This daemonset does not expect to have healthy pods",
                    ),
                }.into(),
            },
            TestHealthCase {
                name: "unhealthy ds with int max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: Some(IntOrString::Int(2)),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(5),
                        desired_number_scheduled: 5,
                        number_ready: 2,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy {
                    status: String::from("DaemonSet unhealthy"),
                    last_error: String::from(
                        "Daemonset 'test': The number of pods ready is less that the desired: 2 < 3",
                    ),
                }.into(),
            },
            TestHealthCase {
                name: "unhealthy ds with percent max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: Some(IntOrString::String("40%".into())),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(5),
                        desired_number_scheduled: 5,
                        number_ready: 2,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy {
                    status: String::from("DaemonSet unhealthy"),
                    last_error: String::from(
                        "Daemonset 'test': The number of pods ready is less that the desired: 2 < 3",
                    ),
                }.into(),
            },
            TestHealthCase {
                name: "healthy ds with int max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: Some(IntOrString::Int(3)),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(5),
                        desired_number_scheduled: 5,
                        number_ready: 2,
                        ..Default::default()
                    }),
                },
                expected: Healthy {
                    status: String::from("DaemonSet 'test' healthy: Pods ready are equal or greater than desired: 2 >= 2"),
                }.into(),
            },
            TestHealthCase {
                name: "healthy ds with percent max_unavailable",
                ds: DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(String::from("test")),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(DaemonSetUpdateStrategies::RollingUpdate.to_string()),
                            rolling_update: Some(RollingUpdateDaemonSet {
                                max_unavailable: Some(IntOrString::String("60%".into())),
                                ..Default::default()
                            }),
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        updated_number_scheduled: Some(5),
                        desired_number_scheduled: 5,
                        number_ready: 2,
                        ..Default::default()
                    }),
                },
                expected: Healthy {
                    status: String::from("DaemonSet 'test' healthy: Pods ready are equal or greater than desired: 2 >= 2"),
                }.into(),
            },
        ];

        for test_case in test_cases {
            let health_run: Result<Health, HealthCheckerError> =
                K8sHealthDaemonSet::check_health_single_daemon_set(test_case.ds);
            let health_result = health_run.unwrap_or_else(|err| {
                panic!(
                    "Test case '{}' is not returning a Health Result: {}",
                    test_case.name, err
                )
            });
            assert_eq!(health_result, test_case.expected);
        }
    }
}
