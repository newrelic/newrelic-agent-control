use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::k8s::{K8s, K8sObject};
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::SubAgentInternalEvent;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::k8s::annotations::Annotations;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::Labels;
use crate::sub_agent::health::health_checker::spawn_health_checker;
use crate::sub_agent::health::k8s::health_checker::SubAgentHealthChecker;
use crate::sub_agent::health::with_start_time::StartTime;
use crate::sub_agent::identity::{AgentIdentity, ID_ATTRIBUTE_NAME};
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::sub_agent::version::k8s::checkers::K8sAgentVersionChecker;
use crate::sub_agent::version::version_checker::spawn_version_checker;
use crate::utils::thread_context::{
    NotStartedThreadContext, StartedThreadContext, ThreadContextStopperError,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::serde_json;
use kube::{api::DynamicObject, core::TypeMeta};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, info_span, trace, warn};

const OBJECTS_SUPERVISOR_INTERVAL_SECONDS: u64 = 30;
const SUPERVISOR_THREAD_NAME: &str = "supervisor";

pub struct NotStartedSupervisorK8s {
    agent_identity: AgentIdentity,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: K8s,
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
        info!("Starting k8s supervisor");
        let resources = Arc::new(self.build_dynamic_objects()?);

        let thread_contexts = vec![
            Some(self.start_k8s_objects_supervisor(resources.clone())),
            self.start_health_check(sub_agent_internal_publisher.clone(), resources.clone())?,
            self.start_version_checker(sub_agent_internal_publisher, resources),
        ];
        info!("K8s supervisor started");

        Ok(StartedSupervisorK8s {
            agent_id: self.agent_identity.id,
            thread_contexts: thread_contexts.into_iter().flatten().collect(),
        })
    }
}

impl NotStartedSupervisorK8s {
    pub fn new(
        agent_identity: AgentIdentity,
        k8s_client: Arc<SyncK8sClient>,
        k8s_config: K8s,
    ) -> Self {
        Self {
            agent_identity,
            k8s_client,
            k8s_config,
            interval: Duration::from_secs(OBJECTS_SUPERVISOR_INTERVAL_SECONDS),
        }
    }

    pub fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorStarterError> {
        self.k8s_config
            .objects
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

        let mut labels = Labels::new(&self.agent_identity.id);
        // Merge default labels with the ones coming from the config with default labels taking precedence.
        labels.append_extra_labels(&k8s_obj.metadata.labels);

        let annotations =
            Annotations::new_agent_type_id_annotation(&self.agent_identity.agent_type_id);

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
    ) -> StartedThreadContext {
        let k8s_client = self.k8s_client.clone();
        let interval = self.interval;
        let agent_id = self.agent_identity.id.clone();

        let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
            let span = info_span!(
                "reconcile_resources",
                { ID_ATTRIBUTE_NAME } = %agent_id
            );
            let _guard = span.enter();

            // Check and apply k8s objects
            if let Err(err) = Self::apply_resources(resources.iter(), k8s_client.clone()) {
                warn!(%err, "K8s resources apply failed");
            }

            // Check the cancellation signal
            if stop_consumer.is_cancelled(interval) {
                break;
            }
        };

        NotStartedThreadContext::new(SUPERVISOR_THREAD_NAME, callback).start()
    }

    pub fn start_health_check(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Arc<Vec<DynamicObject>>,
    ) -> Result<Option<StartedThreadContext>, SupervisorStarterError> {
        let start_time = StartTime::now();

        let Some(health_config) = &self.k8s_config.health else {
            debug!("Health checks are disabled for this agent");
            return Ok(None);
        };

        let Some(k8s_health_checker) =
            SubAgentHealthChecker::try_new(self.k8s_client.clone(), resources, start_time)?
        else {
            warn!("Health checks disabled, there aren't compatible k8s resources to check");
            return Ok(None);
        };

        let started_thread_context = spawn_health_checker(
            self.agent_identity.id.clone(),
            k8s_health_checker,
            sub_agent_internal_publisher,
            health_config.interval,
            start_time,
        );

        Ok(Some(started_thread_context))
    }

    pub fn start_version_checker(
        &self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Arc<Vec<DynamicObject>>,
    ) -> Option<StartedThreadContext> {
        let k8s_version_checker = K8sAgentVersionChecker::checked_new(
            self.k8s_client.clone(),
            &self.agent_identity.id,
            resources,
        )?;

        Some(spawn_version_checker(
            self.agent_identity.id.clone(),
            k8s_version_checker,
            sub_agent_internal_publisher,
            VersionCheckerInterval::default(),
        ))
    }

    /// It applies each of the provided k8s resources to the cluster if it has changed.
    fn apply_resources<'a>(
        resources: impl Iterator<Item = &'a DynamicObject>,
        k8s_client: Arc<SyncK8sClient>,
    ) -> Result<(), SupervisorStarterError> {
        debug!("Applying k8s objects if changed");
        for res in resources {
            trace!("K8s object: {:?}", res);
            k8s_client.apply_dynamic_object_if_changed(res)?;
        }
        debug!("K8s objects applied");
        Ok(())
    }
}

pub struct StartedSupervisorK8s {
    agent_id: AgentID,
    thread_contexts: Vec<StartedThreadContext>,
}

impl SupervisorStopper for StartedSupervisorK8s {
    fn stop(self) -> Result<(), ThreadContextStopperError> {
        // OnK8s this does not delete directly the CR. It will be the garbage collector doing so if needed.
        let mut stop_result = Ok(());
        for thread_context in self.thread_contexts {
            let thread_name = thread_context.thread_name().to_string();
            match thread_context.stop_blocking() {
                Ok(_) => debug!(agent_id = %self.agent_id, "Thread {} stopped", thread_name),
                Err(error_msg) => {
                    error!(agent_id = %self.agent_id, "Error stopping '{thread_name}': {error_msg}");
                    if stop_result.is_ok() {
                        stop_result = Err(error_msg);
                    }
                }
            }
        }

        stop_result
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::agent_control::run::Environment;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::k8s::{K8sHealthConfig, K8sObjectMeta};
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::SubAgentEvent;
    use crate::event::channel::pub_sub;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::hash_repository::repository::tests::MockHashRepository;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssembler;
    use crate::sub_agent::k8s::builder::tests::k8s_sample_runtime_config;
    use crate::sub_agent::remote_config_parser::tests::MockRemoteConfigParser;
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::{NotStartedSubAgent, SubAgent};
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepository;
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::serde_json;
    use kube::api::DynamicObject;
    use kube::core::TypeMeta;
    use predicates::prelude::predicate;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use std::thread;
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
        let agent_identity = AgentIdentity::from((
            AgentID::new("test").unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let mut mock_k8s_client = MockSyncK8sClient::default();
        mock_k8s_client
            .expect_default_namespace()
            .return_const(TEST_NAMESPACE.to_string());

        let mut labels = Labels::new(&agent_identity.id);
        labels.append_extra_labels(&k8s_object().metadata.labels);
        let annotations = Annotations::new_agent_type_id_annotation(&agent_identity.agent_type_id);

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
            agent_identity,
            Arc::new(mock_k8s_client),
            K8s {
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
        let agent_identity = AgentIdentity::from((
            AgentID::new("test").unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));
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
            agent_identity,
            k8s_client: Arc::new(mock_client),
            k8s_config: Default::default(),
        };

        let started_thread_context =
            supervisor.start_k8s_objects_supervisor(Arc::new(vec![dynamic_object()]));
        thread::sleep(Duration::from_millis(300)); // Sleep a bit more than one interval, two apply calls should be executed.
        started_thread_context.stop_blocking().unwrap()
    }

    #[test]
    fn test_start_health_check_fails() {
        let (sub_agent_internal_publisher, _) = pub_sub();
        let config = K8s {
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

        let config = K8s {
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

        let config = K8s {
            objects: HashMap::from([("obj".to_string(), k8s_object())]),
            health: None,
        };

        let not_started = not_started_supervisor(config, None);
        let started = not_started
            .start(sub_agent_internal_publisher)
            .expect("supervisor started");
        assert!(
            !started
                .thread_contexts
                .iter()
                .any(|thread_contexts| thread_contexts.thread_name() == "k8s health checker")
        );
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
        config: K8s,
        additional_expectations_fn: Option<fn(&mut MockSyncK8sClient)>,
    ) -> NotStartedSupervisorK8s {
        let agent_identity = AgentIdentity::from((
            AgentID::new(TEST_AGENT_ID).unwrap(),
            AgentTypeID::try_from(TEST_GENT_FQN).unwrap(),
        ));

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

        NotStartedSupervisorK8s::new(agent_identity, Arc::new(mock_client), config)
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let agent_identity = AgentIdentity::from((
            AgentID::new(TEST_AGENT_ID).unwrap(),
            AgentTypeID::try_from(TEST_GENT_FQN).unwrap(),
        ));

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

        let mut sub_agent_remote_config_hash_repository = MockHashRepository::default();
        sub_agent_remote_config_hash_repository
            .expect_get()
            .with(predicate::eq(agent_identity.id.clone()))
            .return_const(Ok(None));

        let mut yaml_config_repository = MockYAMLConfigRepository::new();
        let yaml_config = YAMLConfig::default();
        let yaml_config_clone = yaml_config.clone();
        yaml_config_repository
            .expect_load_remote()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::always(),
            )
            .return_once(|_, _| Ok(Some(yaml_config_clone)));
        let remote_config_parser = MockRemoteConfigParser::new();

        let mut effective_agents_assembler = MockEffectiveAgentAssembler::new();
        let effective_agent = EffectiveAgent::new(
            agent_identity.clone(),
            Runtime {
                deployment: Deployment::default(),
            },
        );
        effective_agents_assembler.should_assemble_agent(
            &agent_identity,
            &yaml_config,
            &Environment::K8s,
            effective_agent.clone(),
            1,
        );

        let agent_identity_clone = agent_identity.clone();
        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::eq(effective_agent))
            .returning(move |_| {
                Ok(NotStartedSupervisorK8s::new(
                    agent_identity_clone.clone(),
                    mocked_client.clone(),
                    k8s_obj.clone(),
                ))
            });

        SubAgent::new(
            agent_identity,
            Option::<MockStartedOpAMPClient>::None,
            Arc::new(supervisor_builder),
            sub_agent_publisher.into(),
            None,
            (
                sub_agent_internal_publisher.clone(),
                sub_agent_internal_consumer,
            ),
            Arc::new(remote_config_parser),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(yaml_config_repository),
            Arc::new(effective_agents_assembler),
            Environment::K8s,
        )
        .run();

        let timeout = Duration::from_secs(3);

        // sub agent will publish first SubAgentStarted event
        match sub_agent_consumer.as_ref().recv_timeout(timeout).unwrap() {
            SubAgentEvent::SubAgentHealthInfo(_, _) => {
                panic!("SubAgentStarted event expected")
            }
            SubAgentEvent::SubAgentStarted(identity, _) => {
                assert_eq!(identity.id.get(), TEST_AGENT_ID)
            }
        }

        match sub_agent_consumer.as_ref().recv_timeout(timeout).unwrap() {
            SubAgentEvent::SubAgentHealthInfo(_, h) => {
                if h.is_healthy() {
                    panic!("unhealthy event expected")
                }
            }
            SubAgentEvent::SubAgentStarted(_, _) => {
                panic!("unhealthy event expected")
            }
        }
    }
}
