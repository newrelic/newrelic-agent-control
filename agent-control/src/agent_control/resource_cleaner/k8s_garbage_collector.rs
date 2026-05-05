use super::{ResourceCleaner, ResourceCleanerError};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::SubAgentsMap;
use crate::agent_control::defaults::AGENT_CONTROL_ID;
use crate::agent_control::{agent_id::AgentIDError, config::AgentControlConfigError};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::k8s::annotations;
use crate::k8s::client::K8sObjectKey;
use crate::k8s::client::{K8sClient, SyncK8sClient};
use crate::k8s::error::K8sError;
use crate::k8s::labels::{self, AGENT_ID_LABEL_KEY, Labels};
use crate::k8s::utils::{get_name, get_namespace};
use kube::api::{ObjectMeta, TypeMeta};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument, warn};

/// The K8sGarbageCollector is responsible for cleaning up resources in Kubernetes that are
/// no longer needed. In practice, this actually performs the stop and deletion of a sub-agent
/// from Kubernetes, once Agent Control has removed it from its list of active sub-agents.
///
/// It supports two modes of operation, with a public method for each:
/// [`retain`](K8sGarbageCollector::retain) and [`collect`](K8sGarbageCollector::collect).
pub struct K8sGarbageCollector<C: K8sClient = SyncK8sClient> {
    pub k8s_client: Arc<C>,
    /// The namespace where the Agent Control stores data via configMaps.
    pub namespace: String,
    /// The namespace where agents are running. We are garbage collecting resources here only due to Instrumentation
    pub namespace_agents: String,
    pub cr_type_meta: Vec<TypeMeta>,
}

impl<C: K8sClient> K8sGarbageCollector<C> {
    /// Remove all the Kubernetes resources managed by Agent Control that are not included in the
    /// map passed as parameter.
    #[instrument(skip_all, name = "k8s_garbage_collector_retain")]
    pub fn retain(
        &self,
        active_agents: HashMap<AgentID, AgentTypeID>,
    ) -> Result<(), K8sGarbageCollectorError> {
        let mode = K8sGarbageCollectorMode::RetainConfig(&active_agents);
        self.garbage_collect_agent_control_resources(&mode)?;
        self.garbage_collect_sub_agent_resources(&mode, &self.namespace_agents)?;
        self.garbage_collect_sub_agent_resources(&mode, &self.namespace)
    }

    /// Garbage collect resources managed by AC associated to a certain
    /// Agent ID and sub-agent config.
    #[instrument(skip_all, name = "k8s_garbage_collector_collect")]
    pub fn collect(
        &self,
        id: &AgentID,
        agent_type_id: &AgentTypeID,
    ) -> Result<(), K8sGarbageCollectorError> {
        // Do not collect anything if the agent id is the one for Agent Control
        if id == &AgentID::AgentControl {
            return Err(K8sGarbageCollectorError::AgentControlId);
        }

        let mode = K8sGarbageCollectorMode::Collect(id, agent_type_id);
        self.garbage_collect_agent_control_resources(&mode)?;
        self.garbage_collect_sub_agent_resources(&mode, &self.namespace_agents)?;
        self.garbage_collect_sub_agent_resources(&mode, &self.namespace)
    }

    pub fn active_config_ids(active_config: &SubAgentsMap) -> HashMap<AgentID, AgentTypeID> {
        active_config
            .iter()
            .map(|(id, config)| (id.clone(), config.agent_type.clone()))
            .collect()
    }

    fn garbage_collect_agent_control_resources(
        &self,
        mode: &K8sGarbageCollectorMode,
    ) -> Result<(), K8sGarbageCollectorError> {
        // List ConfigMaps by label selector and delete only those owned by Agent Control
        let label_selector_query = mode.label_selector_query();
        debug!("listing ConfigMaps using label selector: `{label_selector_query}`");
        self.k8s_client
            .list_configmaps(&self.namespace, &label_selector_query)?
            .into_iter()
            .filter(|cm| {
                let empty_map = BTreeMap::new();
                let annotations = cm.metadata.annotations.as_ref().unwrap_or(&empty_map);
                annotations::is_owned_by_agent_control(annotations)
            })
            .try_for_each(|cm| {
                if let Some(name) = cm.metadata.name.as_deref() {
                    debug!("deleting agent-control ConfigMap: `{name}`");
                    self.k8s_client.delete_configmap(&self.namespace, name)?;
                } else {
                    warn!("found ConfigMap without name. Skipping deletion.");
                }
                Ok(())
            })
    }

    fn garbage_collect_sub_agent_resources(
        &self,
        mode: &K8sGarbageCollectorMode,
        namespace: &str,
    ) -> Result<(), K8sGarbageCollectorError> {
        // Delete dynamic resources depending on mode
        self.cr_type_meta.iter().try_for_each(|tm| {
            match self.k8s_client.list_dynamic_objects(tm, namespace) {
                Ok(dyn_objs) => {
                    dyn_objs
                        .into_iter()
                        .try_for_each(|d| -> Result<(), K8sGarbageCollectorError> {
                            if self.should_delete_dynamic_object(&d.metadata, mode)? {
                                let name = get_name(&d)?;
                                let namespace = get_namespace(&d)?;

                                debug!("deleting sub-agent resource: `{}/{}`", tm.kind, name);
                                self.k8s_client.delete_dynamic_object(
                                    tm,
                                    K8sObjectKey {
                                        name: &name,
                                        namespace: &namespace,
                                    },
                                )?;
                            }
                            Ok(())
                        })
                }
                Err(K8sError::MissingAPIResource(e)) => {
                    debug!(error = %e, "skipping GC for TypeMeta {}", tm.kind);
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        })?;
        Ok(())
    }

    fn should_delete_dynamic_object(
        &self,
        obj_meta: &ObjectMeta,
        mode: &K8sGarbageCollectorMode,
    ) -> Result<bool, K8sGarbageCollectorError> {
        // I only need to work with references here, so I pre-define an empty BTreeMap which does
        // not allocate anything on its own and use it as default value for labels and annotations
        // in case any of them are None.
        let empty_map = BTreeMap::new();
        let labels = obj_meta.labels.as_ref().unwrap_or(&empty_map);
        let annotations = obj_meta.annotations.as_ref().unwrap_or(&empty_map);

        // We delete resources only if they are managed by Agent Control
        if !labels::is_managed_by_agent_control(labels) {
            return Ok(false);
        }

        // We only delete dynamic objects that are owned by a sub-agent.
        // Agent Control internal resources (e.g. fleet-data ConfigMaps) are handled
        // by garbage_collect_agent_control_resources.
        if !annotations::is_owned_by_sub_agent(annotations) {
            warn!("dynamic object missing owned-by=sub-agent annotation, skipping");
            return Ok(false);
        }

        let agent_id_from_labels = labels::get_agent_id(labels)
            .ok_or(K8sGarbageCollectorError::MissingLabels)?
            .as_str();

        match AgentID::try_from(agent_id_from_labels) {
            Ok(id) => mode.should_delete_agent_id(&id, obj_meta),
            // We must not delete anything with reserved AgentIDs (currently only Agent Control)
            Err(AgentIDError::Reserved(_)) => Ok(false),
            // We should also be conservative, so we do not delete an object if we cannot retrieve a valid AgentID from it
            Err(e) => {
                warn!(
                    namespace = self.namespace,
                    error = %e,
                    "invalid agent id with name {agent_id_from_labels}"
                );
                Ok(false)
            }
        }
    }
}

impl ResourceCleaner for K8sGarbageCollector {
    fn clean(&self, id: &AgentID, agent_type_id: &AgentTypeID) -> Result<(), ResourceCleanerError> {
        // Call the collect method to perform garbage collection.
        self.collect(id, agent_type_id)?;
        Ok(())
    }
}

/// Garbage collector operation modes.
enum K8sGarbageCollectorMode<'a> {
    /// Retain all resources that are in the config map passed as parameter.
    /// Remove all others.
    RetainConfig(&'a HashMap<AgentID, AgentTypeID>),
    /// Remove all resources associated with the Agent ID and sub-agent config passed as parameter.
    Collect(&'a AgentID, &'a AgentTypeID),
}

impl K8sGarbageCollectorMode<'_> {
    fn label_selector_query(&self) -> String {
        let default_label_selector = Labels::default().selector();
        match self {
            K8sGarbageCollectorMode::RetainConfig(active_agents) => format!(
                "{default_label_selector}, {AGENT_ID_LABEL_KEY} notin ({})", //codespell:ignore
                active_agents
                    .iter()
                    // Including the Agent Control ID in the list of IDs to be retained
                    .fold(AGENT_CONTROL_ID.to_string(), |acc, (id, _)| format!(
                        "{acc},{id}"
                    ))
            ),
            K8sGarbageCollectorMode::Collect(agent_id, _) => {
                format!("{default_label_selector}, {AGENT_ID_LABEL_KEY} in ({agent_id})")
            }
        }
    }

    fn should_delete_agent_id(
        &self,
        agent_id: &AgentID,
        obj_meta: &ObjectMeta,
    ) -> Result<bool, K8sGarbageCollectorError> {
        match self {
            K8sGarbageCollectorMode::RetainConfig(agent_identities) => {
                if let Some(agent_type_id) = agent_identities.get(agent_id) {
                    // Check if the agent type is different from the one in the config.
                    // This is to support the case where the agent id exists in the config,
                    // but it's a different agent type. See PR#655 for some details.
                    // Objects without the annotation (e.g. fleet-data ConfigMaps) are not
                    // supervisor-created resources and should not be deleted here.
                    match Self::retrieve_annotated_agent_type_id(obj_meta) {
                        Ok(annotated_agent_type_id) => {
                            Ok(annotated_agent_type_id != agent_type_id.to_string())
                        }
                        Err(K8sGarbageCollectorError::MissingAnnotations) => {
                            warn!("object missing agent type id annotations, skipping");
                            Ok(false)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    // Delete if the agent id does not exist in the passed config
                    Ok(true)
                }
            }

            K8sGarbageCollectorMode::Collect(id, agent_type_id) => {
                if agent_id == *id {
                    // Objects without the annotation (e.g. fleet-data ConfigMaps) are not
                    // supervisor-created resources and should not be deleted here.
                    match Self::retrieve_annotated_agent_type_id(obj_meta) {
                        Ok(annotated_agent_type_id) => {
                            Ok(annotated_agent_type_id == agent_type_id.to_string())
                        }
                        Err(K8sGarbageCollectorError::MissingAnnotations) => {
                            warn!("object missing agent type id annotations, skipping");
                            Ok(false)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(false)
                }
            }
        }
    }

    fn retrieve_annotated_agent_type_id(
        obj_meta: &ObjectMeta,
    ) -> Result<String, K8sGarbageCollectorError> {
        let empty_map = BTreeMap::new();
        let annotations = obj_meta.annotations.as_ref().unwrap_or(&empty_map);
        let annotated_agent_type_id = annotations::get_agent_type_id_value(annotations)
            .ok_or(K8sGarbageCollectorError::MissingAnnotations)?
            .to_owned();
        Ok(annotated_agent_type_id)
    }
}

#[derive(Error, Debug)]
pub enum K8sGarbageCollectorError {
    #[error("the kube client returned an error: {0}")]
    Generic(#[from] K8sError),

    #[error("garbage collector failed loading config store: {0}")]
    LoadingConfigStore(#[from] AgentControlConfigError),

    #[error("garbage collector fetched resources without required labels")]
    MissingLabels,

    #[error("garbage collector fetched resources without required annotations")]
    MissingAnnotations,

    #[error("attempted to clean up resources for Agent Control")]
    AgentControlId,
}

impl From<K8sGarbageCollectorError> for ResourceCleanerError {
    fn from(err: K8sGarbageCollectorError) -> Self {
        Self(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::k8s::annotations::Annotations;

    use super::*;
    use crate::k8s::client::tests::MockK8sClient;
    use k8s_openapi::api::core::v1::ConfigMap;
    use kube::api::{DynamicObject, ObjectMeta};
    use mockall::predicate;

    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_NAMESPACE_AGENTS: &str = "test-namespace-agents";

    #[test]
    fn errors_if_ac_id() {
        let mut k8s_client = MockK8sClient::default();
        // collect should return immediately on AC ID, and return with an error
        k8s_client.expect_list_configmaps().never();
        k8s_client.expect_list_dynamic_objects().never();
        k8s_client.expect_delete_dynamic_object().never();

        let garbage_collector = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        let ac_id = &AgentID::AgentControl;
        let ac_type_id =
            &AgentTypeID::try_from("newrelic/com.newrelic.agent-control:0.0.1").unwrap();

        assert!(matches!(
            garbage_collector.collect(ac_id, ac_type_id),
            Err(K8sGarbageCollectorError::AgentControlId)
        ));
    }

    #[test]
    fn deletes_configmaps_but_not_dynamic_objects() {
        let type_meta = TypeMeta::default();
        let mut k8s_client = MockK8sClient::default();
        // collect should return immediately on AC ID, and return with an error
        k8s_client
            .expect_list_configmaps()
            .once()
            .with(predicate::eq(TEST_NAMESPACE), predicate::eq("app.kubernetes.io/managed-by==newrelic-agent-control, newrelic.io/agent-id in (foo-agent)"))
            .returning(|_, _| Ok(vec![]));
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .with(
                predicate::eq(type_meta.clone()),
                predicate::eq(TEST_NAMESPACE_AGENTS),
            )
            .returning(|_, _| Ok(vec![]));
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .with(
                predicate::eq(type_meta.clone()),
                predicate::eq(TEST_NAMESPACE),
            )
            .returning(|_, _| Ok(vec![]));
        k8s_client.expect_delete_dynamic_object().never();

        let garbage_collector = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        let ac_id = &AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = &AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();

        assert!(garbage_collector.collect(ac_id, agent_type_id).is_ok());
    }

    fn new_dynamic_object(
        agent_id: &AgentID,
        agent_type_id: Option<&AgentTypeID>,
        namespace: &str,
    ) -> Arc<DynamicObject> {
        let annotations = agent_type_id.map(|id| Annotations::new_sub_agent_owned(id).get());
        Arc::new(DynamicObject {
            types: None,
            metadata: ObjectMeta {
                name: Some(format!("fleet-data-{agent_id}")),
                namespace: Some(namespace.to_string()),
                labels: Some(Labels::new(agent_id).get()),
                annotations,
                ..Default::default()
            },
            data: serde_json::Value::Null,
        })
    }

    fn new_dynamic_object_without_owned_by(
        agent_id: &AgentID,
        agent_type_id: Option<&AgentTypeID>,
        namespace: &str,
    ) -> Arc<DynamicObject> {
        let annotations =
            agent_type_id.map(|id| Annotations::new_agent_type_id_annotation(id).get());
        Arc::new(DynamicObject {
            types: None,
            metadata: ObjectMeta {
                name: Some(format!("fleet-data-{agent_id}")),
                namespace: Some(namespace.to_string()),
                labels: Some(Labels::new(agent_id).get()),
                annotations,
                ..Default::default()
            },
            data: serde_json::Value::Null,
        })
    }

    fn mock_k8s_client_listing_objects(
        configmaps: Vec<Arc<ConfigMap>>,
        namespace_agents_objects: Vec<Arc<DynamicObject>>,
        namespace_objects: Vec<Arc<DynamicObject>>,
    ) -> MockK8sClient {
        let type_meta = TypeMeta::default();
        let mut mock = MockK8sClient::default();
        mock.expect_list_configmaps()
            .once()
            .return_once(move |_, _| Ok(configmaps));
        mock.expect_list_dynamic_objects()
            .once()
            .with(
                predicate::eq(type_meta.clone()),
                predicate::eq(TEST_NAMESPACE_AGENTS),
            )
            .return_once(move |_, _| Ok(namespace_agents_objects));
        mock.expect_list_dynamic_objects()
            .once()
            .with(predicate::eq(type_meta), predicate::eq(TEST_NAMESPACE))
            .return_once(move |_, _| Ok(namespace_objects));
        mock
    }

    // RetainConfig mode: object with a matching annotation for an active agent is retained.
    #[test]
    fn retain_skips_dynamic_object_with_matching_annotation() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let cm = new_dynamic_object(&agent_id, Some(&agent_type_id), TEST_NAMESPACE);

        let active_agents = HashMap::from([(agent_id, agent_type_id)]);

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client.expect_delete_dynamic_object().never();

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.retain(active_agents).is_ok());
    }

    // Collect mode: dynamic object whose annotation matches the target type is deleted.
    #[test]
    fn collect_deletes_dynamic_object_with_matching_annotation() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let cm = new_dynamic_object(&agent_id, Some(&agent_type_id), TEST_NAMESPACE);

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client
            .expect_delete_dynamic_object()
            .once()
            .returning(|_, _| Ok(either::Either::Right(kube::core::Status::default())));

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.collect(&agent_id, &agent_type_id).is_ok());
    }

    // RetainConfig mode: object for active agent with NO annotation (fleet-data CM) is skipped.
    #[test]
    fn retain_skips_dynamic_object_without_annotation() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let cm = new_dynamic_object_without_owned_by(&agent_id, None, TEST_NAMESPACE);

        let active_agents = HashMap::from([(agent_id, agent_type_id)]);

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client.expect_delete_dynamic_object().never();

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.retain(active_agents).is_ok());
    }

    // Collect mode: object for matching agent with NO annotation (fleet-data CM) is skipped.
    #[test]
    fn collect_skips_dynamic_object_without_annotation() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let cm = new_dynamic_object_without_owned_by(&agent_id, None, TEST_NAMESPACE);

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client.expect_delete_dynamic_object().never();

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.collect(&agent_id, &agent_type_id).is_ok());
    }

    // RetainConfig mode: object for INACTIVE agent without owned-by annotation is skipped.
    // We are conservative: objects without owned-by=sub-agent are not sub-agent resources.
    #[test]
    fn retain_skips_dynamic_object_for_inactive_agent_without_owned_by() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let cm = new_dynamic_object_without_owned_by(&agent_id, None, TEST_NAMESPACE);

        let active_agents: HashMap<AgentID, AgentTypeID> = HashMap::new();

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client.expect_delete_dynamic_object().never();

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.retain(active_agents).is_ok());
    }

    // RetainConfig mode: object for INACTIVE agent with owned-by=sub-agent is deleted.
    #[test]
    fn retain_deletes_dynamic_object_for_inactive_agent_with_sub_agent_owned_by() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let cm = new_dynamic_object(&agent_id, Some(&agent_type_id), TEST_NAMESPACE);

        let active_agents: HashMap<AgentID, AgentTypeID> = HashMap::new();

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client
            .expect_delete_dynamic_object()
            .once()
            .returning(|_, _| Ok(either::Either::Right(kube::core::Status::default())));

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.retain(active_agents).is_ok());
    }

    // RetainConfig mode: object whose annotation names a different type than the active one
    // must be deleted (the agent type was replaced).
    #[test]
    fn retain_deletes_dynamic_object_with_mismatched_annotation() {
        let type_meta = TypeMeta::default();
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let active_type = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let old_type = AgentTypeID::try_from("newrelic/com.example.bar:0.0.1").unwrap();

        // The object on the cluster carries the OLD type annotation.
        let cm = new_dynamic_object(&agent_id, Some(&old_type), TEST_NAMESPACE);

        let active_agents = HashMap::from([(agent_id, active_type)]);

        let mut k8s_client = mock_k8s_client_listing_objects(vec![], vec![], vec![cm]);
        k8s_client
            .expect_delete_dynamic_object()
            .once()
            .returning(|_, _| Ok(either::Either::Right(kube::core::Status::default())));

        let gc = K8sGarbageCollector {
            k8s_client: Arc::new(k8s_client),
            cr_type_meta: vec![type_meta],
            namespace: TEST_NAMESPACE.to_string(),
            namespace_agents: TEST_NAMESPACE_AGENTS.to_string(),
        };
        assert!(gc.retain(active_agents).is_ok());
    }
}
