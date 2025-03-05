use super::labels::{Labels, AGENT_ID_LABEL_KEY};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{AgentTypeFQN, SubAgentsMap};
use crate::agent_control::config_storer::loader_storer::AgentControlDynamicConfigLoader;
use crate::agent_control::defaults::AGENT_CONTROL_ID;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{pub_sub, EventPublisher};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::error::GarbageCollectorK8sError::{
    MissingActiveAgents, MissingAnnotations, MissingLabels,
};
use crate::k8s::error::{GarbageCollectorK8sError, K8sError};
use crate::k8s::Error::MissingName;
use crate::k8s::{annotations, labels};
use crate::utils::threads::spawn_named_thread;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::TypeMeta;
use std::{sync::Arc, thread, time::Duration};
use tracing::{debug, info, trace, warn};

const GRACEFUL_STOP_RETRY_INTERVAL: Duration = Duration::from_millis(10);

/// Responsible for cleaning resources created by the agent control that are not longer used.
pub struct NotStartedK8sGarbageCollector<S>
where
    S: AgentControlDynamicConfigLoader + Sync + Send + 'static,
{
    config_store: Arc<S>,
    k8s_client: Arc<SyncK8sClient>,
    interval: Duration,
    // None active_config is representing the initial state before the first load.
    active_config: Option<SubAgentsMap>,
    // List of known custom resources to be collected
    cr_type_meta: Vec<TypeMeta>,
}

pub struct K8sGarbageCollectorStarted {
    stop_tx: EventPublisher<CancellationMessage>,
    handle: thread::JoinHandle<()>,
}

impl K8sGarbageCollectorStarted {
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
    fn stop(&self) {
        let _ = self.stop_tx.publish(());
        while !self.handle.is_finished() {
            thread::sleep(GRACEFUL_STOP_RETRY_INTERVAL)
        }
    }
}

impl Drop for K8sGarbageCollectorStarted {
    fn drop(&mut self) {
        self.stop();
    }
}

impl<S> NotStartedK8sGarbageCollector<S>
where
    S: AgentControlDynamicConfigLoader + Sync + Send,
{
    pub fn new(
        config_store: Arc<S>,
        k8s_client: Arc<SyncK8sClient>,
        cr_type_meta: Vec<TypeMeta>,
        interval: Duration,
    ) -> Self {
        NotStartedK8sGarbageCollector {
            config_store,
            k8s_client,
            interval,
            active_config: None,
            cr_type_meta,
        }
    }

    /// Spawns a thread in charge of performing the garbage collection periodically. The thread will be
    /// gracefully shouted down when the returned `K8sGarbageCollectorStarted` gets dropped.
    pub fn start(mut self) -> K8sGarbageCollectorStarted {
        info!(
            "k8s garbage collector started, executed each {} seconds",
            self.interval.as_secs()
        );
        let (stop_tx, stop_rx) = pub_sub();
        let interval = self.interval;

        let handle = spawn_named_thread("Garbage collector", move || {
            loop {
                let _ = self
                    .collect()
                    .inspect_err(|err| warn!("executing garbage collection: {err}"));
                if stop_rx.is_cancelled(interval) {
                    break;
                }
            }
            info!("k8s garbage collector stopped");
        });

        K8sGarbageCollectorStarted { stop_tx, handle }
    }

    /// Garbage collect all resources managed by the SA associated to removed sub-agents.
    /// Collection is stateful, only happens when the list of active agents has changed from
    /// the previous execution.
    pub fn collect(&mut self) -> Result<(), GarbageCollectorK8sError> {
        // check if current active agents differs from previous execution.
        if !self.update_active_config()? {
            trace!("no agents to clean since last execution");
            return Ok(());
        };
        self.collect_config_maps()?;
        self.collect_dynamic_resources()?;

        Ok(())
    }

    fn collect_config_maps(&self) -> Result<(), GarbageCollectorK8sError> {
        let selector_agent_id = Self::garbage_label_selector_agent_id(
            self.active_config.as_ref().ok_or(MissingActiveAgents())?,
        );
        debug!("collecting config_maps: `{selector_agent_id}`");

        self.k8s_client
            .delete_configmap_collection(selector_agent_id.as_str())?;

        Ok(())
    }

    // collect_dynamic_resources iterates through the populated list of possible crs to be deleted.
    // The call to list_dynamic_objects will return the list of objects of a cr_type,
    // but initially it will try to create the dynamic_object_manager if it isn't present.
    // The loop will be broken if an error is returned except if it is a MissingAPIResource error,
    // then it will just continue the collection for the next cr_type_meta since this scenario
    // happens if an agent with unique cr_type_metas is removed and no resources of that type remain.
    fn collect_dynamic_resources(&self) -> Result<(), GarbageCollectorK8sError> {
        self.cr_type_meta.iter().try_for_each(|tm| {
            match self.k8s_client.list_dynamic_objects(tm) {
                Ok(objects) => objects.into_iter().try_for_each(|d| {
                    if self.should_delete_dynamic_object(d.metadata.clone())? {
                        let name = d
                            .metadata
                            .name
                            .as_ref()
                            .ok_or(MissingName(d.types.clone().unwrap_or_default().kind))?;
                        debug!("deleting dynamic_resource: `{}/{}`", tm.kind, name);
                        self.k8s_client.delete_dynamic_object(tm, name.as_str())?;
                    }
                    Ok::<(), GarbageCollectorK8sError>(())
                }),
                Err(K8sError::MissingAPIResource(e)) => {
                    debug!(error = %e, "GC skipping collect for TypeMeta");
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        })?;

        Ok(())
    }

    /// should_delete_dynamic_object checks if the AgentID is "known" and if the agentType is the expected one
    fn should_delete_dynamic_object(
        &self,
        object_metadata: ObjectMeta,
    ) -> Result<bool, GarbageCollectorK8sError> {
        let labels = object_metadata.labels.clone().unwrap_or_default();
        let annotations = object_metadata.annotations.unwrap_or_default();

        // we delete resources only if they are managed by the agentControl
        if !labels::is_managed_by_agentcontrol(&labels) {
            return Ok(false);
        }

        let agent_id = labels::get_agent_id(&labels).ok_or(MissingLabels())?;

        // we do not want to delete anything related to the agentControl by mistake
        if AGENT_CONTROL_ID == agent_id {
            return Ok(false);
        }

        let active_config = self.active_config.clone().ok_or(MissingActiveAgents())?;

        match active_config.get(&AgentID::new(agent_id.as_str())?) {
            None => Ok(true),
            Some(config) => {
                let fqn = AgentTypeID::try_from(
                    annotations::get_agent_fqn_value(&annotations)
                        .ok_or(MissingAnnotations())?
                        .as_str(),
                )?;
                Ok(config.agent_type != fqn)
            }
        }
    }

    /// Loads the latest agents list from the conf store and returns True if differs from
    /// the cached one.
    fn update_active_config(&mut self) -> Result<bool, GarbageCollectorK8sError> {
        let sub_agents_config = Some(self.config_store.load()?.agents);

        // On the first execution self.active_config is None so the list is updated.
        if self.active_config == sub_agents_config {
            Ok(false)
        } else {
            self.active_config = sub_agents_config;
            Ok(true)
        }
    }

    /// Generates a selector that will match config_maps generated by the SA.
    fn garbage_label_selector_agent_id(active_config: &SubAgentsMap) -> String {
        // We add AGENT_CONTROL_ID to prevent removing any resource related to it.
        let id_list = active_config
            .keys()
            .fold(AGENT_CONTROL_ID.to_string(), |acc, id| {
                format!("{},{}", acc, id)
            });

        format!(
            "{},{AGENT_ID_LABEL_KEY} notin ({id_list})", //codespell:ignore
            Labels::default().selector(),
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::NotStartedK8sGarbageCollector;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::{
        default_group_version_kinds, AgentControlDynamicConfig, AgentTypeFQN, SubAgentConfig,
        SubAgentsMap,
    };
    use crate::agent_control::config_storer::loader_storer::MockAgentControlDynamicConfigLoader;
    use crate::agent_control::defaults::AGENT_CONTROL_ID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::k8s::annotations::Annotations;
    use crate::k8s::client::MockSyncK8sClient;
    #[mockall_double::double]
    use crate::k8s::client::SyncK8sClient;
    use crate::k8s::error::GarbageCollectorK8sError;
    use crate::k8s::error::GarbageCollectorK8sError::{
        MissingActiveAgents, MissingAnnotations, MissingLabels,
    };
    use crate::k8s::labels::{Labels, AGENT_ID_LABEL_KEY, MANAGED_BY_KEY, MANAGED_BY_VAL};
    use assert_matches::assert_matches;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_start_executes_collection_as_expected() {
        // Given a config loader to be initialized with one agent and then changed to another
        // during the whole life of the GC.
        let mut cs = MockAgentControlDynamicConfigLoader::new();
        cs.expect_load()
            .once()
            .returning(move || Ok(sub_agents_config("agent-1")));
        // Expect the GC runs more than 10 times if interval is 1ms and runs for at least 100ms.
        cs.expect_load()
            .times(9..)
            .returning(move || Ok(sub_agents_config("agent-2")));

        // Expect to execute collection of resources only twice, one per each agent list variation.
        let mut k8s_client = SyncK8sClient::default();
        k8s_client
            .expect_delete_configmap_collection()
            .times(2)
            .returning(|_| Ok(()));

        let started_gc = NotStartedK8sGarbageCollector::new(
            Arc::new(cs),
            Arc::new(k8s_client),
            Vec::default(),
            Duration::from_millis(1),
        )
        .start();
        std::thread::sleep(Duration::from_millis(100));

        // Expect the gc is correctly stopped
        started_gc.stop();
        assert!(started_gc.is_finished());
    }

    #[test]
    fn test_garbage_label_selector_agent_id() {
        let fqn = "ns/test:1.2.3";
        let agent_id = "agent";
        let labels = Labels::default();
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({},{agent_id})", //codespell:ignore
                labels.selector(),
                AGENT_CONTROL_ID
            ),

            NotStartedK8sGarbageCollector::<MockAgentControlDynamicConfigLoader>::garbage_label_selector_agent_id(
                &SubAgentsMap::from([get_mock_agent_entry(agent_id, fqn)])
            )
        );
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({})", //codespell:ignore
                labels.selector(),
                AGENT_CONTROL_ID
            ),
            NotStartedK8sGarbageCollector::<MockAgentControlDynamicConfigLoader>::garbage_label_selector_agent_id(
                &SubAgentsMap::from([])
            )
        );
    }

    #[test]
    fn test_should_delete_dynamic_object_valid() {
        let test_cases: Vec<(&str, ObjectMeta, bool)> = vec![
            ("no-labels-no-annotations", ObjectMeta::default(), false),
            (
                "agent-control-label-no-annotations",
                ObjectMeta {
                    labels: Some(Labels::new(&AgentID::new_agent_control_id()).get()),
                    ..Default::default()
                },
                false,
            ),
            (
                "random-label-no-annotations",
                ObjectMeta {
                    labels: Some(BTreeMap::from([("test".to_string(), "test2".to_string())])),
                    ..Default::default()
                },
                false,
            ),
            (
                "unknown-id",
                ObjectMeta {
                    labels: Some(Labels::new(&AgentID::new("unknown").unwrap()).get()),
                    ..Default::default()
                },
                true,
            ),
            (
                "known-id-unknown-fqn",
                ObjectMeta {
                    labels: Some(Labels::new(&AgentID::new("test-id").unwrap()).get()),
                    annotations: Some(
                        Annotations::new_agent_fqn_annotation(
                            &AgentTypeID::try_from("ns/unknown:1.2.3").unwrap(),
                        )
                        .get(),
                    ),
                    ..Default::default()
                },
                true,
            ),
            (
                "known-id-known-fqn",
                ObjectMeta {
                    labels: Some(Labels::new(&AgentID::new("test-id").unwrap()).get()),
                    annotations: Some(
                        Annotations::new_agent_fqn_annotation(
                            &AgentTypeID::try_from("ns/test-fqn:1.2.3").unwrap(),
                        )
                        .get(),
                    ),
                    ..Default::default()
                },
                false,
            ),
        ];

        for (name, input, output) in test_cases {
            let test_id = "test-id";
            let test_fqn = "ns/test-fqn:1.2.3";
            let config: SubAgentsMap =
                SubAgentsMap::from([get_mock_agent_entry(test_id, test_fqn)]);

            let gc = NotStartedK8sGarbageCollector {
                config_store: Arc::new(MockAgentControlDynamicConfigLoader::new()),
                k8s_client: Arc::new(MockSyncK8sClient::new()),
                interval: Default::default(),
                active_config: Some(config),
                cr_type_meta: default_group_version_kinds(),
            };

            assert_eq!(
                gc.should_delete_dynamic_object(input).unwrap(),
                output,
                "{name} failed"
            );
        }
    }

    #[test]
    fn test_should_delete_dynamic_object_fail() {
        type TestCase = (
            &'static str,
            ObjectMeta,
            fn(GarbageCollectorK8sError, String),
        );
        let test_cases: Vec<TestCase> = vec![
            (
                "missing-annotations",
                ObjectMeta {
                    labels: Some(Labels::new(&AgentID::new("test-id").unwrap()).get()),
                    annotations: None,
                    ..Default::default()
                },
                |err: GarbageCollectorK8sError, name: String| {
                    assert_matches!(err, MissingAnnotations(), "{}", name)
                },
            ),
            (
                "missing-labels",
                ObjectMeta {
                    labels: Some(BTreeMap::from([(
                        MANAGED_BY_KEY.to_string(),
                        MANAGED_BY_VAL.to_string(),
                    )])),
                    ..Default::default()
                },
                |err: GarbageCollectorK8sError, name: String| {
                    assert_matches!(err, MissingLabels(), "{}", name)
                },
            ),
        ];

        let mut gc = NotStartedK8sGarbageCollector {
            config_store: Arc::new(MockAgentControlDynamicConfigLoader::new()),
            k8s_client: Arc::new(MockSyncK8sClient::new()),
            interval: Default::default(),
            active_config: None,
            cr_type_meta: default_group_version_kinds(),
        };

        assert_matches!(
            gc.should_delete_dynamic_object(ObjectMeta {
                labels: Some(Labels::new(&AgentID::new("unknown").unwrap()).get()),
                ..Default::default()
            })
            .unwrap_err(),
            MissingActiveAgents(..),
            "no active agents"
        );

        for (name, input, test_fn) in test_cases {
            let test_id = "test-id";
            let test_fqn = "ns/test-fqn:1.2.3";
            gc.active_config = Some(SubAgentsMap::from([get_mock_agent_entry(
                test_id, test_fqn,
            )]));

            test_fn(
                gc.should_delete_dynamic_object(input).unwrap_err(),
                name.to_string(),
            );
        }
    }

    // HELPERS
    fn get_mock_agent_entry(agent_id: &str, agent_fqn: &str) -> (AgentID, SubAgentConfig) {
        (
            AgentID::new(agent_id).unwrap(),
            SubAgentConfig {
                agent_type: agent_fqn.try_into().unwrap(),
            },
        )
    }

    fn sub_agents_config(agent_id: &str) -> AgentControlDynamicConfig {
        AgentControlDynamicConfig {
            agents: HashMap::from([(
                AgentID::new(agent_id).unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from("namespace/test:0.0.1").unwrap(),
                },
            )]),
        }
    }
}
