use super::items::{check_health_for_items, flux_release_filter};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::IntOrPercentage;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetStatus, DaemonSetUpdateStrategy};
use std::sync::Arc;

enum UpdateStrategyType {
    OnDelete,
    RollingUpdate,
}

const ROLLING_UPDATE: &str = "RollingUpdate";
const ON_DELETE: &str = "OnDelete";
const DAEMON_SET_KIND: &str = "DaemonSet";

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
}

impl HealthChecker for K8sHealthDaemonSet {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let daemon_sets = self.k8s_client.list_daemon_set();

        let target_daemon_sets = daemon_sets
            .into_iter()
            .filter(flux_release_filter(self.release_name.clone()));

        check_health_for_items(target_daemon_sets, Self::check_health_single_daemon_set)
    }
}

impl K8sHealthDaemonSet {
    pub fn new(k8s_client: Arc<SyncK8sClient>, release_name: String) -> Self {
        Self {
            k8s_client,
            release_name,
        }
    }

    pub fn check_health_single_daemon_set(
        ds: Arc<DaemonSet>,
    ) -> Result<Health, HealthCheckerError> {
        let name = Self::get_daemon_set_name(&ds)?;
        let status = Self::get_daemon_set_status(name.as_str(), &ds)?;
        let update_strategy = Self::get_daemon_set_update_strategy(name.as_str(), &ds)?;

        let update_strategy_type = UpdateStrategyType::try_from(
            Self::get_daemon_set_rolling_update_type(name.as_str(), &update_strategy)?,
        )
        .map_err(|err| HealthCheckerError::InvalidK8sObject {
            kind: DAEMON_SET_KIND.to_string(),
            name: name.to_string(),
            err: format!("unexpected value for .spec.updateStrategy.type: {err}"),
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
                    HealthCheckerError::InvalidK8sObject{
                        kind: DAEMON_SET_KIND.to_string(),
                        name: name.to_string(),
                        err: format!("unexpected value for .spec.updateStrategy.rollingUpdate.maxUnavailable: {err}"),
                    }
                })?
                .scaled_value(status.desired_number_scheduled, true),
        };

        let expected_ready = status.desired_number_scheduled - max_unavailable;
        if status.number_ready < expected_ready {
            return Ok(Self::unhealthy(format!(
                "Daemonset '{}': The number of pods ready is less that the desired: {} < {}",
                name, status.number_ready, expected_ready
            )));
        }

        Ok(Self::healthy(format!(
            "DaemonSet '{}' healthy: Pods ready are equal or greater than desired: {} >= {}",
            name, status.number_ready, expected_ready
        )))
    }

    fn missing_field_error(name: &str, field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField {
            kind: DAEMON_SET_KIND.to_string(),
            name: name.to_string(),
            field: field.to_string(),
        }
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
                DAEMON_SET_KIND.to_string(),
            ))
        })
    }

    fn get_daemon_set_status(
        name: &str,
        daemon_set: &DaemonSet,
    ) -> Result<DaemonSetStatus, HealthCheckerError> {
        daemon_set
            .status
            .clone()
            .ok_or_else(|| Self::missing_field_error(name, ".status"))
    }

    fn get_daemon_set_update_strategy(
        name: &str,
        daemon_set: &DaemonSet,
    ) -> Result<DaemonSetUpdateStrategy, HealthCheckerError> {
        daemon_set
            .spec
            .clone()
            .ok_or_else(|| Self::missing_field_error(name, ".spec"))?
            .update_strategy
            .ok_or_else(|| Self::missing_field_error(name, ".spec.updateStrategy"))
    }

    fn get_daemon_set_rolling_update_type(
        name: &str,
        update_strategy: &DaemonSetUpdateStrategy,
    ) -> Result<String, HealthCheckerError> {
        update_strategy
            .type_
            .clone()
            .ok_or_else(|| Self::missing_field_error(name, ".spec.updateStrategy.Type"))
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::{
        k8s::client::MockSyncK8sClient, sub_agent::health::k8s::health_checker::LABEL_RELEASE_FLUX,
    };
    use assert_matches::assert_matches;
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
                let err_result =
                    K8sHealthDaemonSet::check_health_single_daemon_set(Arc::new(self.ds))
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
                    DAEMON_SET_KIND.to_string(),
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
                expected: HealthCheckerError::InvalidK8sObject {
                    kind: DAEMON_SET_KIND.to_string(),
                    name: daemon_set_name(),
                    err: "unexpected value for .spec.updateStrategy.type: Unknown Update Strategy Type: 'Unknown-TEST'".to_string(),
                },
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
                expected: HealthCheckerError::InvalidK8sObject{
                    kind: DAEMON_SET_KIND.to_string(),
                    name: daemon_set_name(),
                    err: "unexpected value for .spec.updateStrategy.rollingUpdate.maxUnavailable: invalid digit found in string".to_string(),
                },
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
                    K8sHealthDaemonSet::check_health_single_daemon_set(Arc::new(self.ds));
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

    fn daemon_set_name() -> String {
        "test".to_string()
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
                update_strategy: Some(DaemonSetUpdateStrategy {
                    type_: Some(ON_DELETE.to_string()), // on-delete are always healthy
                    ..Default::default()
                }),
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
                number_ready: 2, // There are 3 unavailable, maximum allowed are 2.
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

        let health_checker =
            K8sHealthDaemonSet::new(Arc::new(k8s_client), release_name.to_string());
        let result = health_checker.check_health().unwrap();

        let unhealthy = assert_matches!(
            result,
            Health::Unhealthy(unhealthy) => unhealthy,
            "Expected Unhealthy, got: {:?}",
            result
        );
        assert!(
            unhealthy.last_error().contains("unhealthy-daemon-set"),
            "The unhealthy message should point to the unhealthy daemon-set, got {:?}",
            unhealthy
        );
    }

    fn test_util_get_common_metadata() -> ObjectMeta {
        ObjectMeta {
            name: Some(daemon_set_name()),
            ..Default::default()
        }
    }

    fn test_util_missing_field(field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField {
            kind: DAEMON_SET_KIND.to_string(),
            name: daemon_set_name(),
            field: field.to_string(),
        }
    }
}
