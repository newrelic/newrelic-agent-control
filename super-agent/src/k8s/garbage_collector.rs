use super::{
    error::K8sError,
    labels::{Labels, AGENT_ID_LABEL_KEY},
};
use crate::super_agent::{self, config_storer::storer::SuperAgentConfigLoader};
use crossbeam::{
    channel::{tick, unbounded, Sender},
    select,
};
use std::{collections::BTreeSet, sync::Arc, thread, time::Duration};
use tracing::{debug, info, trace, warn};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;

const DEFAULT_INTERVAL_SEC: u64 = 30;
const GRACEFUL_STOP_RETRY_INTERVAL_MS: u64 = 10;

type ActiveAgents = BTreeSet<String>;

/// Responsible for cleaning resources created by the super agent that are not longer used.
pub struct NotStartedK8sGarbageCollector<S>
where
    S: SuperAgentConfigLoader + std::marker::Sync + std::marker::Send + 'static,
{
    config_store: Arc<S>,
    k8s_client: Arc<SyncK8sClient>,
    interval: Duration,
    // None active_agents is representing the initial state before the first load.
    active_agents: Option<ActiveAgents>,
}

pub struct K8sGarbageCollectorStarted {
    stop_tx: Sender<()>,
    handle: std::thread::JoinHandle<()>,
}

impl K8sGarbageCollectorStarted {
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
    fn stop(&self) {
        let _ = self.stop_tx.send(());
        while !self.handle.is_finished() {
            thread::sleep(Duration::from_millis(GRACEFUL_STOP_RETRY_INTERVAL_MS))
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
    S: SuperAgentConfigLoader + std::marker::Sync + std::marker::Send,
{
    pub fn new(config_store: Arc<S>, k8s_client: Arc<SyncK8sClient>) -> Self {
        NotStartedK8sGarbageCollector {
            config_store,
            k8s_client,
            interval: Duration::from_secs(DEFAULT_INTERVAL_SEC),
            active_agents: None,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Spawns a thread in charge of performing the garbage collection periodically. The thread will be
    /// gracefully shouted down when the returned `K8sGarbageCollectorStarted` gets dropped.
    pub fn start(mut self) -> K8sGarbageCollectorStarted {
        info!(
            "k8s garbage collector started, executed each {} seconds",
            self.interval.as_secs()
        );
        let (stop_tx, stop_rx) = unbounded();
        let ticker = tick(self.interval);

        let handle = std::thread::spawn(move || {
            loop {
                select! {
                    recv(stop_rx)-> _ => break,
                    recv(ticker)-> _ => {
                        let _ = self.collect().inspect_err(|err| warn!("executing garbage collection: {err}"));
                    },
                }
            }
            info!("k8s garbage collector stopped");
        });

        K8sGarbageCollectorStarted { stop_tx, handle }
    }

    /// Garbage collect all resources managed by the SA associated to removed sub-agents.
    /// Collection is stateful, only happens when the list of active agents has changed from
    /// the previous execution.
    pub fn collect(&mut self) -> Result<(), K8sError> {
        // check if current active agents differs from previous execution.
        if !self.update_active_agents()? {
            trace!("no agents to clean since last execution");
            return Ok(());
        };

        let selector = Self::garbage_label_selector(
            self.active_agents
                .as_ref()
                .ok_or(K8sError::MissingActiveAgents())?,
        );

        debug!("collecting resources: `{selector}`");

        // Garbage collect all supported custom resources managed by the SA and associated to sub agents that currently don't exists
        for tm in self.k8s_client.supported_type_meta_collection().into_iter() {
            let _ = self
                .k8s_client
                .delete_dynamic_object_collection(&tm, selector.as_str())
                .inspect_err(|e| warn!("fail trying to delete collection of {:?}: {}", tm, e));
        }

        // Garbage collect CM of identifiers
        self.k8s_client
            .delete_configmap_collection(selector.as_str())?;

        Ok(())
    }

    /// Loads the latest agents list from the conf store and returns True if differs from the
    /// the cached one.
    fn update_active_agents(&mut self) -> Result<bool, K8sError> {
        let super_agent_config = SuperAgentConfigLoader::load(self.config_store.as_ref())?;

        let latest_active_agents =
            Some(super_agent_config.agents.keys().map(|a| a.get()).collect());

        // On the first execution self.active_agents is None so the list is updated.
        if self.active_agents == latest_active_agents {
            Ok(false)
        } else {
            self.active_agents = latest_active_agents;
            Ok(true)
        }
    }

    /// Generates a selector that will match resources generated by the SA for sub-agents not in the list of active_agents.
    fn garbage_label_selector(active_agents: &ActiveAgents) -> String {
        // We add SUPER_AGENT_ID to prevent removing any resource related to it.
        let id_list = active_agents.iter().fold(
            super_agent::defaults::SUPER_AGENT_ID.to_string(),
            |acc, id| format!("{acc},{id}"),
        );

        format!(
            "{},{AGENT_ID_LABEL_KEY} notin ({id_list})",
            Labels::default().selector(),
        )
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::{ActiveAgents, NotStartedK8sGarbageCollector};
    use crate::k8s::labels::{Labels, AGENT_ID_LABEL_KEY};
    use crate::super_agent::config::{
        AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig, SuperAgentConfig,
    };
    use crate::super_agent::config_storer::storer::MockSuperAgentConfigLoader;
    use crate::super_agent::defaults::SUPER_AGENT_ID;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    #[mockall_double::double]
    use crate::k8s::client::SyncK8sClient;

    #[test]
    fn test_start_executes_collection_as_expected() {
        // Given a config loader to be initialized with one agent and then changed to another
        // during the whole life of the GC.
        let mut cs = MockSuperAgentConfigLoader::new();
        cs.expect_load()
            .once()
            .returning(move || Ok(super_agent_config("agent-1")));
        // Expect the GC runs more than 10 times if interval is 1ms and runs for at least 100ms.
        cs.expect_load()
            .times(9..)
            .returning(move || Ok(super_agent_config("agent-2")));

        // Expect to execute collection of resources only twice, one per each agent list variation.
        let mut k8s_client = SyncK8sClient::default();
        k8s_client
            .expect_delete_configmap_collection()
            .times(2)
            .returning(|_| Ok(()));
        k8s_client
            .expect_supported_type_meta_collection()
            .times(2)
            .returning(Vec::new);

        let started_gc = NotStartedK8sGarbageCollector::new(Arc::new(cs), Arc::new(k8s_client))
            .with_interval(Duration::from_millis(1))
            .start();
        std::thread::sleep(Duration::from_millis(100));

        // Expect the gc is correctly stopped
        started_gc.stop();
        assert!(started_gc.is_finished());
    }

    #[test]
    fn test_garbage_label_selector() {
        let agent_id = AgentID::new("agent").unwrap();
        let labels = Labels::default();
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID},{agent_id})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                &ActiveAgents::from([agent_id.get()])
            )
        );
        let second_agent_id = AgentID::new("agent-2").unwrap();
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID},{agent_id},{second_agent_id})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                &ActiveAgents::from([agent_id.get(), second_agent_id.get()])
            )
        );
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                &ActiveAgents::new()
            )
        );
    }

    // HELPERS
    fn super_agent_config(agent_id: &str) -> SuperAgentConfig {
        SuperAgentConfig {
            agents: SubAgentsConfig {
                agents: HashMap::from([(
                    AgentID::new(agent_id).unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeFQN::from("test"),
                    },
                )]),
            },
            ..Default::default()
        }
    }
}
