use std::sync::Arc;

use tracing::{debug, info, info_span};

use crate::{
    agent_type::runtime_config::k8s::K8s,
    event::{SubAgentInternalEvent, channel::EventPublisher},
    sub_agent::{
        effective_agents_assembler::EffectiveAgent,
        k8s::supervisor::{ApplyError, NotStartedSupervisorK8s, StartedSupervisorK8s},
        supervisor::{Supervisor, SupervisorStarter, starter::SupervisorStarterError},
    },
    utils::thread_context::{ThreadCollectionStopperExt, ThreadContextStopperError},
};

impl SupervisorStarter for NotStartedSupervisorK8s {
    type Supervisor = StartedSupervisorK8s;
    type Error = SupervisorStarterError;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::Supervisor, Self::Error> {
        info!("Starting k8s supervisor");
        let resources = Arc::new(self.build_dynamic_objects()?);

        let thread_contexts = [
            Some(self.start_k8s_objects_supervisor(resources.clone())),
            self.start_health_check(sub_agent_internal_publisher.clone(), resources.clone())?,
            self.start_version_checker(sub_agent_internal_publisher.clone(), resources.clone()),
            self.start_guid_checker(sub_agent_internal_publisher.clone(), resources),
        ]
        .into_iter()
        .flatten()
        .collect();
        info!("K8s supervisor started");

        // Reuse structures
        let Self {
            agent_identity,
            k8s_client,
            k8s_config,
            ..
        } = self;

        Ok(StartedSupervisorK8s {
            thread_contexts,
            k8s_client,
            sub_agent_internal_publisher,
            agent_identity,
            k8s_config,
        })
    }
}

impl Supervisor for StartedSupervisorK8s {
    type ApplyError = ApplyError;
    type StopError = ThreadContextStopperError;

    fn apply(self, effective_agent: EffectiveAgent) -> Result<Self, Self::ApplyError> {
        // Retrieve config, if unchanged do nothing
        let new_k8s_config: K8s = effective_agent
            .try_into()
            .map_err(ApplyError::IncomingConfig)?;

        if new_k8s_config == self.k8s_config {
            // No changes, return same supervisor
            return Ok(self);
        }

        debug!(
            agent_id = %self.agent_identity.id,
            "Applying new configuration to K8s supervisor"
        );

        debug!(
            agent_id = %self.agent_identity.id,
            "Stopping old supervisor"
        );

        let Self {
            thread_contexts,
            agent_identity,
            k8s_client,
            sub_agent_internal_publisher,
            ..
        } = self;

        thread_contexts
            .stop()
            .map_err(ApplyError::StoppingPreviousSupervisor)?;

        debug!(agent_id = %agent_identity.id, "Old supervisor stopped");

        // Build a supervisor starter from the new configuration
        let new_starter = NotStartedSupervisorK8s::new(agent_identity, k8s_client, new_k8s_config);

        // and start it
        debug!(agent_id = %new_starter.agent_identity.id, "Starting new supervisor");

        new_starter
            .start(sub_agent_internal_publisher)
            .map_err(ApplyError::StartingSupervisor)
    }

    fn stop(self) -> Result<(), Self::StopError> {
        let span = info_span!("stopping_supervisor", agent_id = %self.agent_identity.id);
        span.in_scope(|| self.thread_contexts.stop())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::k8s::K8sObjectMeta;
    use crate::agent_type::runtime_config::k8s::{K8s, K8sObject};
    use crate::agent_type::runtime_config::rendered::{Deployment, Runtime};
    use crate::event::channel::pub_sub;
    use crate::k8s::annotations::Annotations;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::k8s::labels::Labels;
    use crate::sub_agent::identity::AgentIdentity;
    use crate::sub_agent::supervisor::Supervisor;
    use crate::sub_agent::supervisor::SupervisorStarter;
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::DynamicObject;
    use kube::core::TypeMeta;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    const TEST_API_VERSION: &str = "test/v1";
    const TEST_KIND: &str = "test";
    const TEST_NAMESPACE: &str = "default";
    const TEST_NAME: &str = "test-name";
    const TEST_AGENT_ID: &str = "k8s-test";
    const TEST_GENT_FQN: &str = "ns/test:0.1.2";

    #[test]
    fn test_build_dynamic_objects() {
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test").unwrap(),
            AgentTypeID::try_from("ns/test:0.1.2").unwrap(),
        ));

        let mock_k8s_client = MockSyncK8sClient::default();

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
                ..K8s::default()
            },
        );

        let resources = supervisor.build_dynamic_objects().unwrap();
        assert_eq!(resources, vec![expected.clone(), expected]);
    }

    #[test]
    fn test_k8s_objects_supervisor() {
        let interval = Duration::from_millis(250);
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("test").unwrap(),
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
            health: Some(Default::default()),
            ..K8s::default()
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
            ..K8s::default()
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
            ..K8s::default()
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

    #[test]
    fn test_supervisor_apply() {
        let (sub_agent_internal_publisher, _) = pub_sub();

        let config = K8s {
            objects: HashMap::from([("obj".to_string(), k8s_object())]),
            health: Some(Default::default()),
            ..K8s::default()
        };

        let not_started = not_started_supervisor(config.clone(), None);
        let started = SupervisorStarter::start(not_started, sub_agent_internal_publisher)
            .expect("supervisor started");

        // Apply new config
        let effective_agent = EffectiveAgent::new(
            AgentIdentity::from((
                AgentID::try_from(TEST_AGENT_ID).unwrap(),
                AgentTypeID::try_from(TEST_GENT_FQN).unwrap(),
            )),
            Runtime {
                deployment: Deployment {
                    k8s: Some(config),
                    ..Deployment::default()
                },
            },
        );

        let Ok(started) = started.apply(effective_agent) else {
            // We need to do this because the started supervisor does not implement `Debug`!
            // We could implement that but we'd need to also do it for the nested types which
            // might not have sensible implementations (e.g. thread contexts).
            panic!("supervisor applied");
        };
        Supervisor::stop(started).expect("stopped");
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
                namespace: TEST_NAMESPACE.to_string(),
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
            AgentID::try_from(TEST_AGENT_ID).unwrap(),
            AgentTypeID::try_from(TEST_GENT_FQN).unwrap(),
        ));

        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(|_| Ok(()));
        if let Some(f) = additional_expectations_fn {
            f(&mut mock_client)
        }

        NotStartedSupervisorK8s::new(agent_identity, Arc::new(mock_client), config)
    }
}
