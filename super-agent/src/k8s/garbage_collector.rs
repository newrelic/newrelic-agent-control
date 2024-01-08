use super::{
    error::K8sError,
    labels::{Labels, AGENT_ID_LABEL_KEY},
};
use crate::{
    config::{store::SuperAgentConfigLoader, super_agent_configs::AgentID},
    super_agent,
};
use crossbeam::{
    channel::{tick, unbounded, Sender},
    select,
};
use std::{sync::Arc, thread, time::Duration};
use tracing::{debug, info, warn};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

const DEFAULT_INTERVAL_SEC: u64 = 30;
const GRACEFUL_STOP_RETRY_INTERVAL_MS: u64 = 10;

/// Responsible for cleaning resources created by the super agent that are not longer used.
pub struct NotStartedK8sGarbageCollector<S>
where
    S: SuperAgentConfigLoader + std::marker::Sync + std::marker::Send + 'static,
{
    config_store: Arc<S>,
    k8s_executor: Arc<K8sExecutor>,
    interval: Duration,
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
    pub fn new(config_store: Arc<S>, k8s_executor: Arc<K8sExecutor>) -> Self {
        NotStartedK8sGarbageCollector {
            config_store,
            k8s_executor,
            interval: Duration::from_secs(DEFAULT_INTERVAL_SEC),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Spawns a thread in charge of performing the garbage collection periodically. The thread will be
    /// gracefully shouted down when the returned `K8sGarbageCollectorStarted` gets dropped.
    pub fn start(self) -> K8sGarbageCollectorStarted {
        let (stop_tx, stop_rx) = unbounded();
        let ticker = tick(self.interval);

        let handle = std::thread::spawn(move || {
            info!("k8s garbage collector started");
            loop {
                select! {
                    recv(stop_rx)->_ => break,
                    recv(ticker)->_ => {

                        if let Err(err)=self.collect(){
                            warn!("executing garbage collection: {err}")
                        }
                    }
                }
            }
            info!("k8s garbage collector stopped");
        });

        K8sGarbageCollectorStarted { stop_tx, handle }
    }

    /// Garbage collect all resources managed by the SA associated to removed sub-agents.
    pub fn collect(&self) -> Result<(), K8sError> {
        let super_agent_config = SuperAgentConfigLoader::load(self.config_store.as_ref())?;

        let selector =
            Self::garbage_label_selector(super_agent_config.agents.keys().cloned().collect());

        debug!("collecting resources: `{selector}`");

        // Garbage collect all supported custom resources managed by the SA and associated to sub agents that currently don't exists
        for tm in self.k8s_executor.supported_type_meta_collection().iter() {
            if let Err(e) = crate::runtime::runtime().block_on(
                self.k8s_executor
                    .delete_dynamic_object_collection(tm.clone(), selector.as_str()),
            ) {
                warn!("fail trying to delete collection of {:?}: {e}", tm);
            }
        }

        // Garbage collect CM of identifiers
        crate::runtime::runtime().block_on(
            self.k8s_executor
                .delete_configmap_collection(selector.as_str()),
        )?;

        Ok(())
    }

    fn garbage_label_selector(agent_list: Vec<AgentID>) -> String {
        // We add SUPER_AGENT_ID to prevent removing any resource related to it.
        let id_list = agent_list.iter().fold(
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
    use super::NotStartedK8sGarbageCollector;
    use crate::config::store::MockSuperAgentConfigLoader;
    use crate::config::super_agent_configs::AgentID;
    use crate::k8s::labels::{Labels, AGENT_ID_LABEL_KEY};
    use crate::super_agent::defaults::SUPER_AGENT_ID;
    use std::sync::Arc;
    use std::time::Duration;

    #[mockall_double::double]
    use crate::k8s::executor::K8sExecutor;

    #[test]
    fn test_start_executes_collection_as_expected() {
        let mut cs = MockSuperAgentConfigLoader::new();

        // Expect the gc runs more than 10 times if interval is 1ms and runs for at least 100ms.
        cs.expect_load().times(10..).returning(move || {
            // returning any error for simplicity
            Err(crate::config::error::SuperAgentConfigError::SubAgentNotFound(String::new()))
        });

        let started_gc =
            NotStartedK8sGarbageCollector::new(Arc::new(cs), Arc::new(K8sExecutor::default()))
                .with_interval(Duration::from_millis(1))
                .start();
        std::thread::sleep(Duration::from_millis(100));

        // Expect the gc is correctly stopped
        started_gc.stop();
        assert!(started_gc.is_finished());
    }

    #[test]
    fn test_garbage_label_selector() {
        let agent_id = AgentID::new("test").unwrap();
        let labels = Labels::default();
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID},{agent_id})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                vec![agent_id.clone()]
            )
        );
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID},{agent_id},{agent_id})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                vec![agent_id.clone(), agent_id.clone()]
            )
        );
        assert_eq!(
            format!(
                "{},{AGENT_ID_LABEL_KEY} notin ({SUPER_AGENT_ID})",
                labels.selector(),
            ),
            NotStartedK8sGarbageCollector::<MockSuperAgentConfigLoader>::garbage_label_selector(
                vec![]
            )
        );
    }
}
