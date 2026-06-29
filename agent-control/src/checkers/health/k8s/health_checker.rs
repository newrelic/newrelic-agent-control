//! Health checkers for Kubernetes resources and the aggregate `K8sHealthChecker`.
use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta3_type_meta};
use crate::agent_type::runtime_config::k8s::{K8sHealthCheckDefinition, K8sHealthResourceKind};
use crate::checkers::health::health_checker::{HealthChecker, HealthCheckerError, Healthy};
use crate::checkers::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::k8s::client::{K8sClient, SyncK8sClient};
use kube::api::TypeMeta;
use resources::{
    ResourceFilter, daemon_set::K8sHealthDaemonSet, deployment::K8sHealthDeployment,
    helm_release::K8sHealthHelmRelease, instrumentation::K8sHealthNRInstrumentation,
    stateful_set::K8sHealthStatefulSet,
};
use std::sync::Arc;
use tracing::trace;

/// Per-resource health-check implementations and shared helpers.
pub mod resources;

// This label selector is added in post-render and present no matter the chart we are installing
// https://github.com/fluxcd/helm-controller/blob/main/CHANGELOG.md#090
/// Flux label key (`helm.toolkit.fluxcd.io/name`) identifying the workloads of a Helm release.
pub const LABEL_RELEASE_FLUX: &str = "helm.toolkit.fluxcd.io/name";

/// This enum wraps all the health check implementations related to a Kubernetes resource.
#[derive(Debug)]
pub enum K8sResourceHealthChecker<C: K8sClient = SyncK8sClient> {
    /// Health checker for a Flux HelmRelease custom resource.
    HelmRelease(K8sHealthHelmRelease<C>),
    /// Health checker for a New Relic Instrumentation custom resource.
    NewRelic(K8sHealthNRInstrumentation<C>),
    /// Health checker for a StatefulSet.
    StatefulSet(K8sHealthStatefulSet<C>),
    /// Health checker for a DaemonSet.
    DaemonSet(K8sHealthDaemonSet<C>),
    /// Health checker for a Deployment.
    Deployment(K8sHealthDeployment<C>),
}

impl<C: K8sClient> HealthChecker for K8sResourceHealthChecker<C> {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            K8sResourceHealthChecker::HelmRelease(helm_release) => helm_release.check_health(),
            K8sResourceHealthChecker::NewRelic(nr_instrumentation) => {
                nr_instrumentation.check_health()
            }
            K8sResourceHealthChecker::StatefulSet(stateful_set) => stateful_set.check_health(),
            K8sResourceHealthChecker::DaemonSet(daemon_set) => daemon_set.check_health(),
            K8sResourceHealthChecker::Deployment(deployment) => deployment.check_health(),
        }
    }
}

/// Returns the health-checks corresponding to a type_meta
pub fn health_checkers_for_type_meta<C: K8sClient>(
    type_meta: TypeMeta,
    k8s_client: Arc<C>,
    name: String,
    namespace: String,
    target_namespace: Option<String>,
    start_time: StartTime,
) -> Vec<K8sResourceHealthChecker<C>> {
    // HelmRelease (Flux CR)
    if type_meta == helmrelease_v2_type_meta() {
        let target_namespace = target_namespace.unwrap_or(namespace.clone());

        vec![
            K8sResourceHealthChecker::HelmRelease(K8sHealthHelmRelease::new(
                k8s_client.clone(),
                type_meta,
                name.clone(),
                namespace.clone(),
                start_time,
            )),
            K8sResourceHealthChecker::StatefulSet(K8sHealthStatefulSet::new(
                k8s_client.clone(),
                ResourceFilter::ByFluxLabel(name.clone()),
                start_time,
                target_namespace.clone(),
            )),
            K8sResourceHealthChecker::DaemonSet(K8sHealthDaemonSet::new(
                k8s_client.clone(),
                ResourceFilter::ByFluxLabel(name.clone()),
                start_time,
                target_namespace.clone(),
            )),
            K8sResourceHealthChecker::Deployment(K8sHealthDeployment::new(
                k8s_client.clone(),
                ResourceFilter::ByFluxLabel(name),
                start_time,
                target_namespace,
            )),
        ]
    // Instrumentation (Newrelic CR)
    } else if type_meta == instrumentation_v1beta3_type_meta() {
        vec![K8sResourceHealthChecker::NewRelic(
            K8sHealthNRInstrumentation::new(k8s_client, type_meta, name, namespace, start_time),
        )]
    // No Health-checkers for any other type meta
    } else {
        trace!("No health-checkers for TypeMeta {type_meta:?}");
        vec![]
    }
}

/// Builds the set of health-checkers for a single explicit [`K8sHealthCheckDefinition`].
///
/// `HelmReleaseWorkload` expands into four checkers: the HelmRelease CR itself (looked up by
/// name) plus three workload kinds (StatefulSet, DaemonSet, Deployment) using the Flux label
/// `helm.toolkit.fluxcd.io/name` to discover the workloads belonging to the release.
/// All other kinds produce exactly one checker matched by name.
fn checkers_for_check_definition<C: K8sClient>(
    check: &K8sHealthCheckDefinition,
    k8s_client: Arc<C>,
    start_time: StartTime,
) -> Vec<K8sResourceHealthChecker<C>> {
    let name = check.name.clone();
    let namespace = check.namespace.clone();

    match &check.kind {
        K8sHealthResourceKind::Deployment => vec![K8sResourceHealthChecker::Deployment(
            K8sHealthDeployment::new(
                k8s_client,
                ResourceFilter::ByName(name),
                start_time,
                namespace,
            ),
        )],
        K8sHealthResourceKind::DaemonSet => vec![K8sResourceHealthChecker::DaemonSet(
            K8sHealthDaemonSet::new(
                k8s_client,
                ResourceFilter::ByName(name),
                start_time,
                namespace,
            ),
        )],
        K8sHealthResourceKind::StatefulSet => vec![K8sResourceHealthChecker::StatefulSet(
            K8sHealthStatefulSet::new(
                k8s_client,
                ResourceFilter::ByName(name),
                start_time,
                namespace,
            ),
        )],
        K8sHealthResourceKind::Instrumentation => vec![K8sResourceHealthChecker::NewRelic(
            K8sHealthNRInstrumentation::new(
                k8s_client,
                instrumentation_v1beta3_type_meta(),
                name,
                namespace,
                start_time,
            ),
        )],
        K8sHealthResourceKind::HelmReleaseWorkload => {
            let target_ns = check
                .target_namespace
                .clone()
                .unwrap_or_else(|| namespace.clone());
            vec![
                // HelmRelease CR looked up directly by name+namespace
                K8sResourceHealthChecker::HelmRelease(K8sHealthHelmRelease::new(
                    k8s_client.clone(),
                    helmrelease_v2_type_meta(),
                    name.clone(),
                    namespace,
                    start_time,
                )),
                // Workloads discovered via the Flux label helm.toolkit.fluxcd.io/name=<release>
                K8sResourceHealthChecker::StatefulSet(K8sHealthStatefulSet::new(
                    k8s_client.clone(),
                    ResourceFilter::ByFluxLabel(name.clone()),
                    start_time,
                    target_ns.clone(),
                )),
                K8sResourceHealthChecker::DaemonSet(K8sHealthDaemonSet::new(
                    k8s_client.clone(),
                    ResourceFilter::ByFluxLabel(name.clone()),
                    start_time,
                    target_ns.clone(),
                )),
                K8sResourceHealthChecker::Deployment(K8sHealthDeployment::new(
                    k8s_client,
                    ResourceFilter::ByFluxLabel(name),
                    start_time,
                    target_ns,
                )),
            ]
        }
    }
}

/// This health-checker implementation contains a collection of [HealthChecker] that are queried to provide a
/// unified health value for agents in Kubernetes.
pub struct K8sHealthChecker<HC = K8sResourceHealthChecker>
where
    HC: HealthChecker,
{
    health_checkers: Vec<HC>,
    start_time: StartTime,
}

impl<C: K8sClient> K8sHealthChecker<K8sResourceHealthChecker<C>> {
    /// Builds a [K8sHealthChecker] from an explicit list of health-check definitions.
    ///
    /// Returns `None` when the list is empty (health checking disabled).
    pub fn from_checks_definition(
        k8s_client: Arc<C>,
        checks: &[K8sHealthCheckDefinition],
        start_time: StartTime,
    ) -> Option<Self> {
        let health_checkers: Vec<_> = checks
            .iter()
            .flat_map(|check| {
                checkers_for_check_definition::<C>(check, k8s_client.clone(), start_time)
            })
            .collect();

        if health_checkers.is_empty() {
            return None;
        }
        Some(Self {
            health_checkers,
            start_time,
        })
    }

    /// Builds a [`K8sHealthChecker`] from a ready-made list of resource health checkers.
    pub fn new(health_checkers: Vec<K8sResourceHealthChecker<C>>, start_time: StartTime) -> Self {
        Self {
            health_checkers,
            start_time,
        }
    }
}

impl<HC> HealthChecker for K8sHealthChecker<HC>
where
    HC: HealthChecker,
{
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        for rhc in self.health_checkers.iter() {
            let health = rhc.check_health()?;
            if !health.is_healthy() {
                return Ok(health);
            }
        }
        Ok(HealthWithStartTime::from_healthy(
            Healthy::new(),
            self.start_time,
        ))
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use crate::agent_type::runtime_config::k8s::{K8sHealthCheckDefinition, K8sHealthResourceKind};
    use crate::checkers::health::health_checker::HealthChecker;
    use crate::checkers::health::health_checker::tests::MockHealthCheck;
    use crate::checkers::health::k8s::health_checker::{
        K8sHealthChecker, K8sResourceHealthChecker,
    };
    use crate::checkers::health::with_start_time::StartTime;
    use crate::k8s::client::tests::MockK8sClient;
    use assert_matches::assert_matches;
    use rstest::rstest;
    use std::sync::Arc;

    impl<HC: HealthChecker> K8sHealthChecker<HC> {
        pub fn checkers_count(&self) -> usize {
            self.health_checkers.len()
        }
    }

    fn check(kind: K8sHealthResourceKind) -> K8sHealthCheckDefinition {
        K8sHealthCheckDefinition {
            name: "test-resource".to_string(),
            namespace: "test-namespace".to_string(),
            kind,
            target_namespace: None,
        }
    }

    #[test]
    fn no_checks_returns_none() {
        let mock_client = MockK8sClient::default();
        assert!(
            K8sHealthChecker::from_checks_definition(Arc::new(mock_client), &[], StartTime::now())
                .is_none()
        )
    }

    #[rstest]
    #[case::no_target_namespace(vec![K8sHealthCheckDefinition {
            name: "test-resource".to_string(),
            namespace: "test-namespace".to_string(),
            kind: K8sHealthResourceKind::HelmReleaseWorkload,
            target_namespace: None,
        }], "test-namespace", "test-namespace")]
    #[case::some_target_namespace(vec![K8sHealthCheckDefinition {
            name: "test-resource".to_string(),
            namespace: "test-namespace".to_string(),
            kind: K8sHealthResourceKind::HelmReleaseWorkload,
            target_namespace: Some("test-target-namespace".to_string()),
        }], "test-namespace", "test-target-namespace")]
    fn helmrelease_workload_creates_four_checkers(
        #[case] check_definitions: Vec<K8sHealthCheckDefinition>,
        #[case] expected_namespace: &str,
        #[case] expected_target_namespace: &str,
    ) {
        let mock_client = MockK8sClient::new();
        let start_time = StartTime::now();

        let health_checker = K8sHealthChecker::from_checks_definition(
            Arc::new(mock_client),
            &check_definitions,
            start_time,
        )
        .expect("health checker should not be empty");

        assert_eq!(health_checker.health_checkers.len(), 4);
        assert_matches!(
            &health_checker.health_checkers[0],
            K8sResourceHealthChecker::HelmRelease(h) => {
                assert_eq!(h.namespace(), expected_namespace);
            }
        );
        assert_matches!(
            &health_checker.health_checkers[1],
            K8sResourceHealthChecker::StatefulSet(s) => {
                assert_eq!(s.namespace(), expected_target_namespace);
            }
        );
        assert_matches!(
            &health_checker.health_checkers[2],
            K8sResourceHealthChecker::DaemonSet(d) => {
                assert_eq!(d.namespace(), expected_target_namespace);
            }
        );
        assert_matches!(
            &health_checker.health_checkers[3],
            K8sResourceHealthChecker::Deployment(d) => {
                assert_eq!(d.namespace(), expected_target_namespace)
            }
        );
    }

    #[test]
    fn instrumentation_creates_one_checker() {
        let mock_client = MockK8sClient::default();
        let start_time = StartTime::now();

        let health_checker = K8sHealthChecker::from_checks_definition(
            Arc::new(mock_client),
            &[check(K8sHealthResourceKind::Instrumentation)],
            start_time,
        )
        .expect("health checker should not be empty");

        assert_eq!(health_checker.health_checkers.len(), 1);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::NewRelic(_)
        );
    }

    #[test]
    fn deployment_creates_one_checker() {
        let mock_client = MockK8sClient::default();
        let health_checker = K8sHealthChecker::from_checks_definition(
            Arc::new(mock_client),
            &[check(K8sHealthResourceKind::Deployment)],
            StartTime::now(),
        )
        .expect("health checker should not be empty");

        assert_eq!(health_checker.health_checkers.len(), 1);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::Deployment(_)
        );
    }

    #[test]
    fn daemon_set_creates_one_checker() {
        let mock_client = MockK8sClient::default();
        let health_checker = K8sHealthChecker::from_checks_definition(
            Arc::new(mock_client),
            &[check(K8sHealthResourceKind::DaemonSet)],
            StartTime::now(),
        )
        .expect("health checker should not be empty");

        assert_eq!(health_checker.health_checkers.len(), 1);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::DaemonSet(_)
        );
    }

    #[test]
    fn stateful_set_creates_one_checker() {
        let mock_client = MockK8sClient::default();
        let health_checker = K8sHealthChecker::from_checks_definition(
            Arc::new(mock_client),
            &[check(K8sHealthResourceKind::StatefulSet)],
            StartTime::now(),
        )
        .expect("health checker should not be empty");

        assert_eq!(health_checker.health_checkers.len(), 1);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::StatefulSet(_)
        );
    }

    #[test]
    fn logic_health_check() {
        let start_time = StartTime::now();
        assert!(
            K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_healthy()
                ],
                start_time,
            }
            .check_health()
            .unwrap()
            .is_healthy()
        );

        assert!(
            !K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_unhealthy(),
                    MockHealthCheck::new_healthy()
                ],
                start_time
            }
            .check_health()
            .unwrap()
            .is_healthy() //Notice that this assert has a ! at the beginning
        );

        assert!(
            K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_with_error(),
                    MockHealthCheck::new_healthy()
                ],
                start_time
            }
            .check_health()
            .is_err()
        );
    }
}
