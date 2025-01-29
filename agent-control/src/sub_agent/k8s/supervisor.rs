use crate::agent_control::config::{AgentID, AgentTypeFQN};
use crate::agent_type::runtime_config;
use crate::agent_type::runtime_config::K8sObject;
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::channel::{pub_sub, EventPublisher, EventPublisherError};
use crate::event::SubAgentInternalEvent;
use crate::k8s::annotations::Annotations;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::Labels;
use crate::sub_agent::health::health_checker::spawn_health_checker;
use crate::sub_agent::health::k8s::health_checker::SubAgentHealthChecker;
use crate::sub_agent::health::with_start_time::StartTime;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::sub_agent::version::k8s::checkers::K8sAgentVersionChecker;
use crate::sub_agent::version::version_checker::spawn_version_checker;
use crate::utils::threads::spawn_named_thread;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::serde_json;
use kube::{api::DynamicObject, core::TypeMeta};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::{debug, error, info, warn};

const OBJECTS_SUPERVISOR_INTERVAL_SECONDS: u64 = 30;

pub struct NotStartedSupervisorK8s {
    agent_id: AgentID,
    agent_fqn: AgentTypeFQN,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: runtime_config::K8s,
    interval: Duration,
}

impl SupervisorStarter for NotStartedSupervisorK8s {
    type SupervisorStopper = StartedSupervisorK8s;

    /// Starts the supervisor, it will periodically:
    /// * Check and update the corresponding k8s resources
    /// * Check health
    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<StartedSupervisorK8s, SupervisorStarterError> {
        let resources = Arc::new(self.build_dynamic_objects()?);

        let (stop_objects_supervisor, objects_supervisor_handle) =
            self.start_k8s_objects_supervisor(resources.clone());
        let maybe_stop_health =
            self.start_health_check(sub_agent_internal_publisher.clone(), resources.clone())?;
        let maybe_stop_version =
            self.start_version_checker(sub_agent_internal_publisher, resources);

        Ok(StartedSupervisorK8s {
            agent_id: self.agent_id,
            maybe_stop_health,
            maybe_stop_version,
            stop_objects_supervisor,
            objects_supervisor_handle,
        })
    }
}

impl NotStartedSupervisorK8s {
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

    pub fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorStarterError> {
        self.k8s_config
            .objects
            .clone()
            .values()
            .map(|k8s_obj| self.create_dynamic_object(k8s_obj))
            .collect()
    }

    fn create_dynamic_object(
        &self,
        k8s_obj: &K8sObject,
    ) -> Result<DynamicObject, SupervisorStarterError> {
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
            SupervisorStarterError::ConfigError(format!("Error serializing fields: {}", e))
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

        info!(%agent_id, "k8s objects supervisor started");
        let join_handle = spawn_named_thread("K8s objects supervisor", move || loop {
            // Check and apply k8s objects
            if let Err(err) = Self::apply_resources(&agent_id, resources.iter(), k8s_client.clone())
            {
                error!(%agent_id, %err, "k8s resources apply failed");
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
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Arc<Vec<DynamicObject>>,
    ) -> Result<Option<EventPublisher<()>>, SupervisorStarterError> {
        let start_time = StartTime::now();

        if let Some(health_config) = self.k8s_config.health.clone() {
            let (stop_health_publisher, stop_health_consumer) = pub_sub();
            let Some(k8s_health_checker) =
                SubAgentHealthChecker::try_new(self.k8s_client.clone(), resources, start_time)?
            else {
                warn!(agent_id=%self.agent_id, "health-check cannot start even if it is enabled there are no compatible k8s resources");
                return Ok(None);
            };

            spawn_health_checker(
                self.agent_id.clone(),
                k8s_health_checker,
                stop_health_consumer,
                sub_agent_internal_publisher,
                health_config.interval,
                start_time,
            );
            return Ok(Some(stop_health_publisher));
        }

        debug!(%self.agent_id, "health checks are disabled for this agent");
        Ok(None)
    }

    pub fn start_version_checker(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Arc<Vec<DynamicObject>>,
    ) -> Option<EventPublisher<()>> {
        let (stop_version_publisher, stop_version_consumer) = pub_sub();

        let k8s_version_checker = K8sAgentVersionChecker::checked_new(
            self.k8s_client.clone(),
            &self.agent_id,
            resources,
        )?;

        spawn_version_checker(
            self.agent_id.clone(),
            k8s_version_checker,
            stop_version_consumer,
            sub_agent_internal_publisher,
            VersionCheckerInterval::default(),
        );
        Some(stop_version_publisher)
    }

    /// It applies each of the provided k8s resources to the cluster if it has changed.
    fn apply_resources<'a>(
        agent_id: &AgentID,
        resources: impl Iterator<Item = &'a DynamicObject>,
        k8s_client: Arc<SyncK8sClient>,
    ) -> Result<(), SupervisorStarterError> {
        debug!(%agent_id, "applying k8s objects if changed");
        for res in resources {
            debug!("K8s object: {:?}", res);
            k8s_client.apply_dynamic_object_if_changed(res)?;
        }
        debug!(%agent_id, "K8s objects applied");
        Ok(())
    }
}

pub struct StartedSupervisorK8s {
    agent_id: AgentID,
    maybe_stop_health: Option<EventPublisher<()>>,
    maybe_stop_version: Option<EventPublisher<()>>,
    stop_objects_supervisor: EventPublisher<()>,
    objects_supervisor_handle: JoinHandle<()>,
}

impl SupervisorStopper for StartedSupervisorK8s {
    fn stop(self) -> Result<(), EventPublisherError> {
        // OnK8s this does not delete directly the CR. It will be the garbage collector doing so if needed.

        if let Some(stop_health) = self.maybe_stop_health {
            stop_health.publish(())?; // TODO: should we also wait the health-check join handle?
        }
        if let Some(stop_version) = self.maybe_stop_version {
            stop_version.publish(())?;
        }

        self.stop_objects_supervisor.publish(())?;
        let _ = self.objects_supervisor_handle.join().inspect_err(|_| {
            error!(
                agent_id = self.agent_id.to_string(),
                "Error stopping k8s supervisor thread"
            );
        });
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::config::{
        helmrelease_v2_type_meta, AgentID, AgentTypeFQN, SubAgentConfig,
    };
    use crate::agent_type::environment::Environment;
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::agent_type::runtime_config::{Deployment, K8sObject, Runtime};
    use crate::event::channel::pub_sub;
    use crate::event::SubAgentEvent;
    use crate::k8s::error::K8sError;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidatorMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::event_handler::opamp::remote_config_handler::RemoteConfigHandler;
    use crate::sub_agent::k8s::builder::tests::k8s_sample_runtime_config;
    use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::{NotStartedSubAgent, SubAgent};
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
    use crate::{agent_type::runtime_config::K8sObjectMeta, k8s::client::MockSyncK8sClient};
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::serde_json;
    use kube::api::DynamicObject;
    use kube::core::TypeMeta;
    use predicates::prelude::predicate;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use std::time::Duration;
    use tracing_test::traced_test;

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

        let supervisor = NotStartedSupervisorK8s::new(
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

        let supervisor = NotStartedSupervisorK8s {
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
                    types: Some(helmrelease_v2_type_meta()),
                    metadata: Default::default(), // missing name
                    data: Default::default(),
                }]),
            )
            .err()
            .unwrap(); // cannot use unwrap_err because the  underlying EventPublisher doesn't implement Debug
        assert_matches!(err, SupervisorStarterError::HealthError(_))
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
            .start(sub_agent_internal_publisher)
            .expect("supervisor started");
        started.stop().expect("supervisor thread joined");
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
            .start(sub_agent_internal_publisher)
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
    ) -> NotStartedSupervisorK8s {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_default_namespace()
            .return_const(TEST_NAMESPACE.to_string());
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(|_| Ok(()));
        if let Some(f) = additional_expectations_fn {
            f(&mut mock_client)
        }

        NotStartedSupervisorK8s::new(agent_id, agent_fqn, Arc::new(mock_client), config)
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

        let mut k8s_obj = k8s_sample_runtime_config(true);
        k8s_obj.health = Some(K8sHealthConfig {
            interval: Duration::from_millis(500).into(),
        });

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(|_| Ok(()));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());
        mock_client.expect_get_dynamic_object().returning(|_, _| {
            Ok(Some(Arc::new(DynamicObject {
                types: Some(helmrelease_v2_type_meta()),
                metadata: Default::default(),
                data: Default::default(),
            })))
        });
        let mocked_client = Arc::new(mock_client);

        let agent_cfg = SubAgentConfig {
            agent_type: agent_fqn.clone(),
        };
        let k8s_config = k8s_sample_runtime_config(true);
        let runtime_config = Runtime {
            deployment: Deployment {
                k8s: Some(k8s_config),
                ..Default::default()
            },
        };
        let effective_agent =
            EffectiveAgent::new(agent_id.clone(), agent_fqn.clone(), runtime_config.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::K8s,
            effective_agent,
            1,
        );

        let mut sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        sub_agent_remote_config_hash_repository
            .expect_get()
            .with(predicate::eq(agent_id.clone()))
            .return_const(Ok(None));
        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let agent_id_clone = agent_id.clone();
        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::always())
            .returning(move |_| {
                Ok(NotStartedSupervisorK8s::new(
                    agent_id_clone.clone(),
                    agent_fqn.clone(),
                    mocked_client.clone(),
                    k8s_obj.clone(),
                ))
            });

        let hash_repository_ref = Arc::new(sub_agent_remote_config_hash_repository);

        let signature_validator = MockRemoteConfigValidatorMock::new();
        let remote_config_handler = RemoteConfigHandler::new(
            agent_id.clone(),
            agent_cfg.clone(),
            hash_repository_ref.clone(),
            Arc::new(remote_values_repo),
            Arc::new(signature_validator),
        );

        let supervisor_assembler = SupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::K8s,
        );

        SubAgent::new(
            AgentID::new(TEST_AGENT_ID).unwrap(),
            agent_cfg.clone(),
            none_mock_opamp_client(),
            supervisor_assembler,
            sub_agent_publisher,
            None,
            (
                sub_agent_internal_publisher.clone(),
                sub_agent_internal_consumer,
            ),
            remote_config_handler,
        )
        .run();

        let timeout = Duration::from_secs(3);

        match sub_agent_consumer.as_ref().recv_timeout(timeout).unwrap() {
            SubAgentEvent::SubAgentHealthInfo(_, _, h) => {
                if h.is_healthy() {
                    panic!("unhealthy event expected")
                }
            }
        }
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
