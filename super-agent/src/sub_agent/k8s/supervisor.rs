use crate::agent_type::runtime_config;
use crate::agent_type::runtime_config::K8sObject;
use crate::event::channel::{pub_sub, EventPublisher, EventPublisherError};
use crate::event::SubAgentInternalEvent;
use crate::k8s::annotations::Annotations;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::error::K8sError;
use crate::k8s::labels::Labels;
use crate::sub_agent::health::health_checker::{
    publish_health_event, spawn_health_checker, HealthCheckerError, Unhealthy,
};
use crate::sub_agent::health::k8s::health_checker::SubAgentHealthChecker;
use crate::super_agent::config::{AgentID, AgentTypeFQN};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::serde_json;
use kube::{api::DynamicObject, core::TypeMeta};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::{debug, error, info, trace};

const OBJECTS_SUPERVISOR_INTERVAL_SECONDS: u64 = 30;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("building k8s resources: `{0}`")]
    ConfigError(String),

    #[error("building health checkers: `{0}`")]
    HealthError(#[from] HealthCheckerError),
}

pub struct NotStartedSupervisor {
    agent_id: AgentID,
    agent_fqn: AgentTypeFQN,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: runtime_config::K8s,
    interval: Duration,
}

impl NotStartedSupervisor {
    pub fn new(
        agent_id: AgentID,
        agent_fqn: AgentTypeFQN,
        k8s_client: Arc<SyncK8sClient>,
        k8s_config: runtime_config::K8s,
    ) -> Self {
        Self {
            agent_id,
            k8s_client,
            k8s_config,
            agent_fqn,
            interval: Duration::from_secs(OBJECTS_SUPERVISOR_INTERVAL_SECONDS),
        }
    }

    /// Starts the supervisor, it will periodically:
    /// * Check and update the corresponding k8s resources
    /// * Check health
    pub fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        start_time: SystemTime,
    ) -> Result<StartedSupervisor, SupervisorError> {
        let resources = Arc::new(self.build_dynamic_objects()?);

        let (stop_objects_supervisor, objects_supervisor_handle) =
            self.start_k8s_objects_supervisor(resources.clone());
        let maybe_stop_health =
            self.start_health_check(sub_agent_internal_publisher, resources, start_time)?;

        Ok(StartedSupervisor {
            maybe_stop_health,
            stop_objects_supervisor,
            objects_supervisor_handle,
        })
    }

    pub fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorError> {
        self.k8s_config
            .objects
            .clone()
            .values()
            .map(|k8s_obj| self.create_dynamic_object(k8s_obj))
            .collect()
    }

    fn create_dynamic_object(&self, k8s_obj: &K8sObject) -> Result<DynamicObject, SupervisorError> {
        let types = TypeMeta {
            api_version: k8s_obj.api_version.clone(),
            kind: k8s_obj.kind.clone(),
        };

        let mut labels = Labels::new(&self.agent_id);
        // Merge default labels with the ones coming from the config with default labels taking precedence.
        labels.append_extra_labels(&k8s_obj.metadata.labels);

        let annotations = Annotations::new_agent_fqn_annotation(&self.agent_fqn);

        let metadata = ObjectMeta {
            name: Some(k8s_obj.metadata.name.clone()),
            namespace: Some(self.k8s_client.default_namespace().to_string()),
            labels: Some(labels.get()),
            annotations: Some(annotations.get()),
            ..Default::default()
        };

        let data = serde_json::to_value(&k8s_obj.fields).map_err(|e| {
            SupervisorError::ConfigError(format!("Error serializing fields: {}", e))
        })?;

        Ok(DynamicObject {
            types: Some(types),
            metadata,
            data,
        })
    }

    fn start_k8s_objects_supervisor(
        &self,
        resources: Arc<Vec<DynamicObject>>,
    ) -> (EventPublisher<()>, JoinHandle<()>) {
        let (stop_publisher, stop_consumer) = pub_sub();
        let interval = self.interval;
        let agent_id = self.agent_id.clone();
        let k8s_client = self.k8s_client.clone();

        let join_handle = thread::spawn(move || loop {
            // Check and apply k8s objects
            if let Err(err) = Self::apply_resources(&agent_id, resources.iter(), k8s_client.clone())
            {
                error!(%err, "k8s resources apply failed");
            }
            // Check the cancellation signal
            if stop_consumer.is_cancelled(interval) {
                info!(%agent_id, "k8s objects supervisor stopped");
                break;
            }
        });

        (stop_publisher, join_handle)
    }

    pub fn start_health_check(
        &self,
        health_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Arc<Vec<DynamicObject>>,
        start_time: SystemTime,
    ) -> Result<Option<EventPublisher<()>>, SupervisorError> {
        if let Some(health_config) = self.k8s_config.health.clone() {
            let (stop_health_publisher, stop_health_consumer) = pub_sub();
            let k8s_health_checker =
                SubAgentHealthChecker::try_new(self.k8s_client.clone(), resources, start_time)?;

            spawn_health_checker(
                self.agent_id.clone(),
                k8s_health_checker,
                stop_health_consumer,
                health_publisher,
                health_config.interval,
                start_time,
            );
            return Ok(Some(stop_health_publisher));
        }

        debug!(%self.agent_id, "health checks are disabled for this agent");
        Ok(None)
    }

    /// It applies each of the provided k8s resources to the cluster if it has changed.
    fn apply_resources<'a>(
        agent_id: &AgentID,
        resources: impl Iterator<Item = &'a DynamicObject>,
        k8s_client: Arc<SyncK8sClient>,
    ) -> Result<(), SupervisorError> {
        debug!(%agent_id, "applying k8s objects if changed");
        for res in resources {
            trace!("K8s object: {:?}", res);
            k8s_client.apply_dynamic_object_if_changed(res)?;
        }
        debug!(%agent_id, "K8s objects applied");
        Ok(())
    }
}

pub struct StartedSupervisor {
    maybe_stop_health: Option<EventPublisher<()>>,
    stop_objects_supervisor: EventPublisher<()>,
    objects_supervisor_handle: JoinHandle<()>,
}

impl StartedSupervisor {
    pub fn stop(self) -> Result<Vec<JoinHandle<()>>, EventPublisherError> {
        if let Some(stop_health) = self.maybe_stop_health {
            stop_health.publish(())?; // TODO: should we also return the health-check join handle?
        }
        self.stop_objects_supervisor.publish(())?;
        Ok(vec![self.objects_supervisor_handle])
    }
}

/// Logs the provided error and publishes the corresponding unhealthy event.
pub fn log_and_report_unhealthy(
    sub_agent_internal_publisher: &EventPublisher<SubAgentInternalEvent>,
    err: &SupervisorError,
    msg: &str,
    start_time: SystemTime,
) {
    let last_error = format!("{msg}: {err}");

    let event = SubAgentInternalEvent::AgentBecameUnhealthy(
        Unhealthy::new(String::default(), last_error),
        start_time,
    );

    error!(%err, msg);
    publish_health_event(sub_agent_internal_publisher, event);
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::agent_type::runtime_config::K8sObject;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::super_agent::config::helm_release_type_meta;
    use crate::{agent_type::runtime_config::K8sObjectMeta, k8s::client::MockSyncK8sClient};
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::serde_json;
    use kube::core::TypeMeta;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};

    const TEST_API_VERSION: &str = "test/v1";
    const TEST_KIND: &str = "test";
    const TEST_NAMESPACE: &str = "default";
    const TEST_NAME: &str = "test-name";

    const TEST_AGENT_ID: &str = "k8s-test";
    const TEST_GENT_FQN: &str = "ns/test:0.1.2";

    #[test]
    fn test_build_dynamic_objects() {
        let agent_id = AgentID::new("test").unwrap();
        let agent_fqn = AgentTypeFQN::try_from("ns/test:0.1.2").unwrap();

        let mut mock_k8s_client = MockSyncK8sClient::default();
        mock_k8s_client
            .expect_default_namespace()
            .return_const(TEST_NAMESPACE.to_string());

        let mut labels = Labels::new(&agent_id);
        labels.append_extra_labels(&k8s_object().metadata.labels);
        let annotations = Annotations::new_agent_fqn_annotation(&agent_fqn);

        let expected = DynamicObject {
            types: Some(TypeMeta {
                api_version: TEST_API_VERSION.to_string(),
                kind: TEST_KIND.to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(labels.get()),
                annotations: Some(annotations.get()),
                ..Default::default()
            },
            data: json!({}),
        };

        let supervisor = NotStartedSupervisor::new(
            agent_id,
            agent_fqn,
            Arc::new(mock_k8s_client),
            runtime_config::K8s {
                objects: HashMap::from([
                    ("mock_cr1".to_string(), k8s_object()),
                    ("mock_cr2".to_string(), k8s_object()),
                ]),
                health: None,
            },
        );

        let resources = supervisor.build_dynamic_objects().unwrap();
        assert_eq!(resources, vec![expected.clone(), expected]);
    }

    #[test]
    fn test_k8s_objects_supervisor() {
        let interval = Duration::from_millis(250);
        let agent_id = AgentID::new("test").unwrap();
        let agent_fqn = AgentTypeFQN::try_from("ns/test:0.1.2").unwrap();
        let apply_issue = "some issue";

        // The first apply call is OK, the second fails
        let mut seq = mockall::Sequence::new();
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Ok(()))
            .in_sequence(&mut seq);
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Err(K8sError::GetDynamic(apply_issue.to_string())))
            .in_sequence(&mut seq);

        let supervisor = NotStartedSupervisor {
            interval,
            agent_id,
            agent_fqn,
            k8s_client: Arc::new(mock_client),
            k8s_config: Default::default(),
        };

        let (stop_ch, join_handle) =
            supervisor.start_k8s_objects_supervisor(Arc::new(vec![dynamic_object()]));
        thread::sleep(Duration::from_millis(300)); // Sleep a bit more than one interval, two apply calls should be executed.
        stop_ch.publish(()).unwrap();
        join_handle.join().unwrap();
    }

    #[test]
    fn test_start_health_check_fails() {
        let (sub_agent_internal_publisher, _) = pub_sub();
        let config = runtime_config::K8s {
            objects: HashMap::from([("obj".to_string(), k8s_object())]),
            health: Some(K8sHealthConfig {
                ..Default::default()
            }),
        };

        let supervisor = not_started_supervisor(config, None);
        let err = supervisor
            .start_health_check(
                sub_agent_internal_publisher,
                Arc::new(vec![DynamicObject {
                    types: Some(helm_release_type_meta()),
                    metadata: Default::default(), // missing name
                    data: Default::default(),
                }]),
                SystemTime::UNIX_EPOCH,
            )
            .err()
            .unwrap(); // cannot use unwrap_err because the  underlying EventPublisher doesn't implement Debug
        assert_matches!(err, SupervisorError::HealthError(_))
    }

    #[test]
    fn test_supervisor_start_stop() {
        let (sub_agent_internal_publisher, _) = pub_sub();

        let config = runtime_config::K8s {
            objects: HashMap::from([("obj".to_string(), k8s_object())]),
            health: Some(Default::default()),
        };

        let not_started = not_started_supervisor(config, None);
        let started = not_started
            .start(sub_agent_internal_publisher, SystemTime::UNIX_EPOCH)
            .expect("supervisor started");
        let _ = started
            .stop()
            .expect("supervisor stopped")
            .into_iter()
            .map(|jh| jh.join().unwrap());
    }

    #[test]
    fn test_supervisor_start_without_health_check() {
        let (sub_agent_internal_publisher, _) = pub_sub();

        let config = runtime_config::K8s {
            objects: HashMap::from([("obj".to_string(), k8s_object())]),
            health: None,
        };

        let not_started = not_started_supervisor(config, None);
        let started = not_started
            .start(sub_agent_internal_publisher, SystemTime::UNIX_EPOCH)
            .expect("supervisor started");
        assert!(started.maybe_stop_health.is_none());
    }

    fn k8s_object() -> K8sObject {
        K8sObject {
            api_version: TEST_API_VERSION.to_string(),
            kind: TEST_KIND.to_string(),
            metadata: K8sObjectMeta {
                labels: BTreeMap::from([
                    ("custom-label".to_string(), "values".to_string()),
                    (
                        AGENT_ID_LABEL_KEY.to_string(),
                        "to be overwritten".to_string(),
                    ),
                ]),
                name: TEST_NAME.to_string(),
            },
            ..Default::default()
        }
    }

    fn dynamic_object() -> DynamicObject {
        DynamicObject {
            types: Some(TypeMeta {
                api_version: TEST_API_VERSION.to_string(),
                kind: TEST_KIND.to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            data: json!({}),
        }
    }

    fn not_started_supervisor(
        config: runtime_config::K8s,
        additional_expectations_fn: Option<fn(&mut MockSyncK8sClient)>,
    ) -> NotStartedSupervisor {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_default_namespace()
            .return_const(TEST_NAMESPACE.to_string());
        if let Some(f) = additional_expectations_fn {
            f(&mut mock_client)
        }

        NotStartedSupervisor::new(agent_id, agent_fqn, Arc::new(mock_client), config)
    }
}
