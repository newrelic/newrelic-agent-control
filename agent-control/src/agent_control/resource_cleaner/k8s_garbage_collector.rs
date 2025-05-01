use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use kube::api::TypeMeta;
use thiserror::Error;
use tracing::{debug, instrument};

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::SubAgentsMap;
use crate::agent_control::defaults::AGENT_CONTROL_ID;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::k8s::annotations;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::error::K8sError;
use crate::k8s::labels::{self, Labels, AGENT_ID_LABEL_KEY};
use crate::{
    agent_control::{agent_id::AgentIDError, config::AgentControlConfigError},
    agent_type::agent_type_id::AgentTypeIDError,
};

use super::{ResourceCleaner, ResourceCleanerError};

/// The K8sGarbageCollector is responsible for cleaning up resources in Kubernetes that are
/// no longer needed. In practice, this actually performs the stop and deletion of a sub-agent
/// from Kubernetes, once Agent Control has removed it from its list of active sub-agents.
///
/// It supports two modes of operation, with a public method for each:
/// [`retain`](K8sGarbageCollector::retain) and [`collect`](K8sGarbageCollector::collect).
pub struct K8sGarbageCollector {
    pub k8s_client: Arc<SyncK8sClient>,
    pub cr_type_meta: Vec<TypeMeta>,
}

impl K8sGarbageCollector {
    /// Remove all the Kubernetes resources managed by Agent Control that are not included in the
    /// map passed as parameter.
    #[instrument(skip_all, name = "k8s_garbage_collector_retain")]
    pub fn retain(
        &self,
        active_agents: HashMap<AgentID, AgentTypeID>,
    ) -> Result<(), GarbageCollectorK8sError> {
        self.garbage_collection(
            |labels, annotations| {
                let agent_id_from_labels =
                    labels::get_agent_id(labels).ok_or(GarbageCollectorK8sError::MissingLabels)?;

                let agent_id_from_labels = match AgentID::new(agent_id_from_labels) {
                    Ok(id) => id,
                    // We must not delete anything with reserved AgentIDs (currently only Agent Control)
                    Err(AgentIDError::Reserved(_)) => return Ok(false),
                    Err(e) => return Err(e.into()),
                };
                match active_agents.get(&agent_id_from_labels) {
                    None => Ok(true), // Delete if the agent id does not exist in the passed config
                    Some(agent_type_id) => {
                        let agent_type_id_from_annotations =
                            annotations::get_agent_type_id_value(annotations)
                                .ok_or(GarbageCollectorK8sError::MissingAnnotations)?;
                        let agent_type_id_from_annotations =
                            AgentTypeID::try_from(agent_type_id_from_annotations.as_str())?;
                        Ok(agent_type_id != &agent_type_id_from_annotations)
                    }
                }
            },
            |default_label_selector| {
                format!(
                    "{default_label_selector}, {AGENT_ID_LABEL_KEY} notin ({})", //codespell:ignore
                    active_agents
                        .iter()
                        // Including the Agent Control ID in the list of IDs to be retained
                        .fold(AGENT_CONTROL_ID.to_string(), |acc, (id, _)| {
                            format!("{acc},{id}")
                        })
                )
            },
        )
    }

    /// Garbage collect resources managed by AC associated to a certain
    /// Agent ID and sub-agent config.
    #[instrument(skip_all, name = "k8s_garbage_collector_collect")]
    pub fn collect(
        &self,
        id: &AgentID,
        agent_type_id: &AgentTypeID,
    ) -> Result<(), GarbageCollectorK8sError> {
        // Do not collect anything if the agent id is the one for Agent Control
        if id.is_agent_control_id() {
            return Err(GarbageCollectorK8sError::AgentControlId);
        }

        self.garbage_collection(
            |labels, annotations| {
                let agent_id_from_labels =
                    labels::get_agent_id(labels).ok_or(GarbageCollectorK8sError::MissingLabels)?;

                let agent_id_from_labels = match AgentID::new(agent_id_from_labels) {
                    Ok(id) => id,
                    // We must not delete anything with reserved AgentIDs (currently only Agent Control)
                    Err(AgentIDError::Reserved(_)) => return Ok(false),
                    Err(e) => return Err(e.into()),
                };
                let agent_type_id_from_annotations =
                    annotations::get_agent_type_id_value(annotations)
                        .ok_or(GarbageCollectorK8sError::MissingAnnotations)?;
                let agent_type_id_from_annotations =
                    AgentTypeID::try_from(agent_type_id_from_annotations.as_str())?;
                Ok(id == &agent_id_from_labels && agent_type_id == &agent_type_id_from_annotations)
            },
            |default_label_selector| {
                format!("{default_label_selector}, {AGENT_ID_LABEL_KEY} in ({id})")
            },
        )
    }

    fn garbage_collection(
        &self,
        should_delete_fn: impl Fn(
            &BTreeMap<String, String>,
            &BTreeMap<String, String>,
        ) -> Result<bool, GarbageCollectorK8sError>,
        label_selector_builder: impl Fn(String) -> String,
    ) -> Result<(), GarbageCollectorK8sError> {
        let default_label_selector = Labels::default().selector();
        let label_selector_query = label_selector_builder(default_label_selector);

        debug!("Deleting configmaps using label selector: `{label_selector_query}`",);
        self.k8s_client
            .delete_configmap_collection(&label_selector_query)?;

        self.cr_type_meta
            .iter()
            .try_for_each(|tm| match self.k8s_client.list_dynamic_objects(tm) {
                Ok(dyn_objs) => dyn_objs.into_iter().try_for_each(|d| {
                    // I only need to work with references here, so I pre-define an empty BTreeMap
                    // which does no allocate anything on its own and use it as default value for
                    // labels and annotations in case any of them are None.
                    let empty = BTreeMap::new();
                    let labels = d.metadata.labels.as_ref().unwrap_or(&empty);
                    let annotations = d.metadata.annotations.as_ref().unwrap_or(&empty);

                    if labels::is_managed_by_agentcontrol(labels)
                        && should_delete_fn(labels, annotations)?
                    {
                        let name = d.metadata.name.as_ref().ok_or_else(|| {
                            K8sError::MissingName(d.types.clone().unwrap_or_default().kind)
                        })?;
                        debug!("deleting dynamic_resource: `{}/{}`", tm.kind, name);
                        self.k8s_client.delete_dynamic_object(tm, name.as_str())?;
                    }
                    Ok(())
                }),
                Err(K8sError::MissingAPIResource(e)) => {
                    debug!(error = %e, "GC skipping for TypeMeta {}", tm.kind);
                    Ok(())
                }
                Err(e) => Err(e.into()),
            })
    }

    pub fn active_config_ids(active_config: &SubAgentsMap) -> HashMap<AgentID, AgentTypeID> {
        active_config
            .iter()
            .map(|(id, config)| (id.clone(), config.agent_type.clone()))
            .collect()
    }
}

impl ResourceCleaner for K8sGarbageCollector {
    fn clean(
        &self,
        id: &AgentID,
        agent_type_id: &AgentTypeID,
    ) -> Result<(), super::ResourceCleanerError> {
        // Call the collect method to perform garbage collection.
        self.collect(id, agent_type_id)?;
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum GarbageCollectorK8sError {
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("garbage collector failed loading config store: `{0}`")]
    LoadingConfigStore(#[from] AgentControlConfigError),

    #[error("garbage collector fetched resources without required labels")]
    MissingLabels,

    #[error("garbage collector fetched resources without required annotations")]
    MissingAnnotations,

    #[error("unable to parse AgentTypeID: `{0}`")]
    ParsingAgentType(#[from] AgentTypeIDError),

    #[error("unable to parse AgentID: `{0}`")]
    ParsingAgentId(#[from] AgentIDError),

    #[error("attempted to clean up resources for Agent Control")]
    AgentControlId,
}

impl From<GarbageCollectorK8sError> for ResourceCleanerError {
    fn from(err: GarbageCollectorK8sError) -> Self {
        Self(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use super::*;

    #[test]
    fn errors_if_ac_id() {
        let mut k8s_client = SyncK8sClient::default();
        // collect should return immediately on AC ID, and return with an error
        k8s_client.expect_delete_configmap_collection().never();
        k8s_client.expect_list_dynamic_objects().never();
        k8s_client.expect_delete_dynamic_object().never();

        let garbage_collector = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![],
        };
        let ac_id = &AgentID::new_agent_control_id();
        let ac_type_id =
            &AgentTypeID::try_from("newrelic/com.newrelic.agent-control:0.0.1").unwrap();

        assert!(matches!(
            garbage_collector.collect(ac_id, ac_type_id),
            Err(GarbageCollectorK8sError::AgentControlId)
        ));
    }

    #[test]
    fn deletes_configmaps_but_not_dynamic_objects() {
        let type_meta = TypeMeta::default();
        let mut k8s_client = SyncK8sClient::default();
        // collect should return immediately on AC ID, and return with an error
        k8s_client
            .expect_delete_configmap_collection()
            .once()
            .with(predicate::eq("app.kubernetes.io/managed-by==newrelic-agent-control, newrelic.io/agent-id in (foo-agent)"))
            .returning(|_| Ok(()));
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .with(predicate::eq(type_meta.clone()))
            .returning(|_| Ok(vec![]));
        k8s_client.expect_delete_dynamic_object().never();

        let garbage_collector = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
        };
        let ac_id = &AgentID::new("foo-agent").unwrap();
        let agent_type_id = &AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();

        assert!(garbage_collector.collect(ac_id, agent_type_id).is_ok());
    }
}
