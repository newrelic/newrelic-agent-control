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
                    status: format!("Daemonset '{name}' has on delete upgrade strategy"),
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

        let int_or_percentage = match rolling_update.max_unavailable {
            // If max unavailable is not set, the daemonset does not expect to have healthy pods.
            // Returning Healthiness as soon as possible.
            None => return Ok(Healthy {
                status: format!(
                    "DaemonSet '{name}' healthy: This daemonset does not expect to have healthy pods",
                ),
            }.into()),
            Some(value) => IntOrPercentage::try_from(value).map_err(|err| {
                HealthCheckerError::new(format!(
                    "Daemonset '{name}' has an non-parsable Max Availability on Update Strategy: '{err}'"
                ))
            })?,
        };

        let max_unavailable = match int_or_percentage {
            // `rolling_update.max_unavailable` can me an integer (number of pods) or a percent (percent of
            // desired pods).

            // The integer path is simple: Number of pods unavailable.
            IntOrPercentage::Int(i) => i,

            // The percent path needs to calculate the percent against the number of desired pods to know
            // how many unavailable pods should be the maximum.
            IntOrPercentage::Percentage(percent) => {
                (status.desired_number_scheduled as f32 * percent).ceil() as i32
            }
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

    #[test]
    fn test_daemonset_spec_errors() {
        struct TestCase {
            name: &'static str,
            ds: DaemonSet,
            expected: HealthCheckerError,
        }

        impl TestCase {
            fn run(self) {
                let err_result = K8sHealthDaemonSet::check_health_single_daemon_set(self.ds)
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
                expected: HealthCheckerError::new("Daemonset has no .metadata.name".into()),
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
                expected: HealthCheckerError::new("Daemonset 'test' has no spec".into()),
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
                expected: HealthCheckerError::new("Daemonset 'test' has no update strategy".into()),
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
                expected: HealthCheckerError::new("Daemonset 'test' has an unknown Update Strategy Type: 'Unknown-TEST'".into()),
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
                expected: HealthCheckerError::new("Daemonset 'test' has rolling update strategy type and no struct".into()),
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
                HealthCheckerError::new("Daemonset 'test' has an non-parsable Max Availability on Update Strategy: 'invalid digit found in string'".into()),
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
                expected: HealthCheckerError::new("Daemonset 'test' has rolling update strategy type and no struct".into()),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_daemonset_healthiness() {
        #[derive(Debug)]
        struct TestCase {
            name: &'static str,
            ds: DaemonSet,
            expected: Health,
        }

        impl TestCase {
            fn run(self) {
                let health_run: Result<Health, HealthCheckerError> =
                    K8sHealthDaemonSet::check_health_single_daemon_set(self.ds);
                let health_result = health_run.unwrap_or_else(|err| {
                    panic!(
                        "Test case '{}' is not returning a Health Result: {}",
                        self.name, err
                    )
                });
                assert_eq!(health_result, self.expected, "{}", self.name);
            }
        }

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
                expected: Unhealthy{
                    status: String::from("DaemonSet unhealthy"),
                    last_error: String::from("Daemonset 'test' is so new that it has no status yet")
                }.into(),
            },
            TestCase {
                name: "ds has on delete update strategy type",
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
                expected: Healthy{
                    status: String::from("Daemonset 'test' has on delete upgrade strategy")
                }.into(),
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
                expected: Unhealthy{
                    status: String::from("DaemonSet unhealthy"),
                    last_error: String::from("Daemonset 'test' is so new that it has no `updated_number_scheduled` status yet")
                }.into(),
            },
            TestCase {
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
            TestCase {
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
            TestCase {
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
            TestCase {
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
            TestCase {
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
            TestCase {
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

        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
