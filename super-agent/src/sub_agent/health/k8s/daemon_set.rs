use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetStatus, DaemonSetUpdateStrategy};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::IntOrPercentage;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use std::sync::Arc;

use super::health_checker::LABEL_RELEASE_FLUX;

enum UpdateStrategyType {
    OnDelete,
    RollingUpdate,
}

const ROLLING_UPDATE: &str = "RollingUpdate";
const ON_DELETE: &str = "OnDelete";

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("Unknown Update Strategy Type: '{0}'")]
pub struct UnknownUpdateStrategyType(String);

impl TryFrom<String> for UpdateStrategyType {
    type Error = UnknownUpdateStrategyType;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            ROLLING_UPDATE => Ok(Self::RollingUpdate),
            ON_DELETE => Ok(Self::OnDelete),
            s => Err(UnknownUpdateStrategyType(s.to_string())),
        }
    }
}

impl std::fmt::Display for UpdateStrategyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let result = match self {
            UpdateStrategyType::RollingUpdate => ROLLING_UPDATE,
            UpdateStrategyType::OnDelete => ON_DELETE,
        };

        write!(f, "{}", result)
    }
}

pub struct K8sHealthDaemonSet {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
    // TODO Waiting on https://github.com/kube-rs/kube/pull/1482, Hardcoding label for now
    // label_selector: String,
}

impl HealthChecker for K8sHealthDaemonSet {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let daemon_set_list = self.k8s_client.list_daemon_set()?;

        let filtered_list_daemon_set = daemon_set_list.into_iter().filter(|ds| {
            crate::k8s::utils::contains_label_with_value(
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
        let name = Self::get_daemon_set_name(&ds)?;
        let status = Self::get_daemon_set_status(&ds)?;
        let update_strategy = Self::get_daemon_set_update_strategy(&ds)?;
        let update_strategy_type = UpdateStrategyType::try_from(
            Self::get_daemon_set_rolling_update_type(&ds)?,
        )
        .map_err(|err| {
            HealthCheckerError::InvalidK8sObjectField(
                ".spec.updateStrategy.type".to_string(),
                name.clone(),
                err.to_string(),
            )
        })?;

        let rolling_update = match update_strategy_type {
            // If the update strategy is not a rolling update, there will be nothing to wait for
            UpdateStrategyType::OnDelete => {
                return Ok(Self::healthy(format!(
                    "Daemonset '{name}' has on delete upgrade strategy"
                )));
            }
            UpdateStrategyType::RollingUpdate => {
                update_strategy.rolling_update.ok_or_else(|| {
                    Self::missing_field_error(name.as_str(), ".spec.updateStrategy.rollingUpdate")
                })?
            }
        };

        if status.updated_number_scheduled.is_none() {
            return Ok(Self::unhealthy(format!(
                "Daemonset '{name}' is so new that it has no `updated_number_scheduled` status yet"
            )));
        }

        // Make sure all the updated pods have been scheduled
        if let Some(updated_number_scheduled) = status.updated_number_scheduled {
            if updated_number_scheduled != status.desired_number_scheduled {
                return Ok(Self::unhealthy(format!(
                    "DaemonSet '{name}' Not all the pods of the were able to schedule"
                )));
            }
        }

        let max_unavailable = match rolling_update.max_unavailable {
            // If max unavailable is not set, the daemon set does not expect to have healthy pods.
            // Returning Healthiness as soon as possible.
            None => {
                return Ok(Self::healthy(format!(
                "DaemonSet '{name}' healthy: This daemon set does not expect to have healthy pods",
            )))
            }
            Some(value) => IntOrPercentage::try_from(value)
                .map_err(|err| {
                    HealthCheckerError::InvalidK8sObjectField(
                        ".spec.updateStrategy.rollingUpdate.maxUnavailable".to_string(),
                        name.clone(),
                        err.to_string(),
                    )
                })?
                .scaled_value(status.desired_number_scheduled, true),
        };

        let expected_ready = status.desired_number_scheduled - max_unavailable;
        if status.number_ready < expected_ready {
            return Ok(Self::unhealthy(format!(
                "Daemonset '{name}': The number of pods ready is less that the desired: {} < {}",
                status.number_ready, expected_ready
            )));
        }

        Ok(Self::healthy(format!(
            "DaemonSet '{name}' healthy: Pods ready are equal or greater than desired: {} >= {}",
            status.number_ready, expected_ready
        )))
    }

    fn missing_field_error(name: &str, field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField(
            field.to_string(),
            format!("Daemonset '{}'", name),
        )
    }

    fn healthy(s: String) -> Health {
        Healthy { status: s }.into()
    }

    fn unhealthy(error: String) -> Health {
        Unhealthy {
            status: "".to_string(),
            last_error: error,
        }
        .into()
    }

    fn get_daemon_set_name(daemon_set: &DaemonSet) -> Result<String, HealthCheckerError> {
        daemon_set.metadata.name.clone().ok_or_else(|| {
            HealthCheckerError::K8sError(crate::k8s::error::K8sError::MissingName(
                "DaemonSet".to_string(),
            ))
        })
    }

    fn get_daemon_set_status(
        daemon_set: &DaemonSet,
    ) -> Result<DaemonSetStatus, HealthCheckerError> {
        let name = Self::get_daemon_set_name(daemon_set)?;

        daemon_set
            .status
            .clone()
            .ok_or_else(|| Self::missing_field_error(name.as_str(), ".status"))
    }

    fn get_daemon_set_update_strategy(
        daemon_set: &DaemonSet,
    ) -> Result<DaemonSetUpdateStrategy, HealthCheckerError> {
        let name = Self::get_daemon_set_name(daemon_set)?;

        daemon_set
            .spec
            .clone()
            .ok_or_else(|| Self::missing_field_error(name.as_str(), ".spec"))?
            .update_strategy
            .ok_or_else(|| Self::missing_field_error(name.as_str(), ".spec.updateStrategy"))
    }

    fn get_daemon_set_rolling_update_type(
        daemon_set: &DaemonSet,
    ) -> Result<String, HealthCheckerError> {
        let name = Self::get_daemon_set_name(daemon_set)?;

        Self::get_daemon_set_update_strategy(daemon_set)?
            .type_
            .clone()
            .ok_or_else(|| Self::missing_field_error(name.as_str(), ".spec.updateStrategy.Type"))
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
    fn test_daemon_set_spec_errors() {
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
                expected: HealthCheckerError::K8sError(crate::k8s::error::K8sError::MissingName(
                    "DaemonSet".to_string(),
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
            TestCase {
                name: "ds without spec",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: None,
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: test_util_missing_field(".spec"),
            },
            TestCase {
                name: "ds without update strategy",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: None,
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: test_util_missing_field(".spec.updateStrategy"),
            },
            TestCase {
                name: "ds with unknown update strategy",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
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
                expected: HealthCheckerError::InvalidK8sObjectField(
                    ".spec.updateStrategy.type".into(),
                    "test".into(),
                    "Unknown Update Strategy Type: 'Unknown-TEST'".into(),
                ),
            },
            TestCase {
                name: "ds which update strategy is rolling but has no struct",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
                            rolling_update: None,
                        }),
                        ..Default::default()
                    }),
                    status: Some(DaemonSetStatus {
                        ..Default::default()
                    }),
                },
                expected: test_util_missing_field(".spec.updateStrategy.rollingUpdate"),
            },
            TestCase {
                name: "ds update strategy policy has non-parsable max_unavailable",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                expected: HealthCheckerError::InvalidK8sObjectField(
                    ".spec.updateStrategy.rollingUpdate.maxUnavailable".into(),
                    "test".into(),
                    "invalid digit found in string".into(),
                ),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_daemon_set_health() {
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
                name: "ds has on delete update strategy type",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ON_DELETE.to_string()),
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
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy{
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                    status: String::from(""),
                    last_error: String::from("Daemonset 'test' is so new that it has no `updated_number_scheduled` status yet")
                }.into(),
            },
            TestCase {
                name: "ds with no unschedulable pods",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                    status: String::from(""),
                    last_error: String::from(
                        "DaemonSet 'test' Not all the pods of the were able to schedule",
                    ),
                }.into(),
            },
            TestCase {
                name: "ds without max_unavailable",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                        "DaemonSet 'test' healthy: This daemon set does not expect to have healthy pods",
                    ),
                }.into(),
            },
            TestCase {
                name: "unhealthy ds with int max_unavailable",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                    status: String::from(""),
                    last_error: String::from(
                        "Daemonset 'test': The number of pods ready is less that the desired: 2 < 3",
                    ),
                }.into(),
            },
            TestCase {
                name: "unhealthy ds with percent max_unavailable",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                    status: String::from(""),
                    last_error: String::from(
                        "Daemonset 'test': The number of pods ready is less that the desired: 2 < 3",
                    ),
                }.into(),
            },
            TestCase {
                name: "healthy ds with int max_unavailable",
                ds: DaemonSet {
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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
                    metadata: test_util_get_common_metadata(),
                    spec: Some(DaemonSetSpec {
                        update_strategy: Some(DaemonSetUpdateStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
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

    fn test_util_get_common_metadata() -> ObjectMeta {
        ObjectMeta {
            name: Some(String::from("test")),
            ..Default::default()
        }
    }

    fn test_util_missing_field(field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField(field.to_string(), "Daemonset 'test'".to_string())
    }
}
