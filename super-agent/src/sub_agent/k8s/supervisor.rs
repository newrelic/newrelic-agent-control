use crate::agent_type::runtime_config;
use crate::agent_type::runtime_config::K8sObject;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentInternalEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::error::K8sError;
use crate::k8s::labels::Labels;
use crate::sub_agent::health::health_checker::{spawn_health_checker, HealthCheckerError};
use crate::sub_agent::health::k8s::health_checker::SubAgentHealthChecker;
use crate::super_agent::config::AgentID;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::serde_json;
use kube::{api::DynamicObject, core::TypeMeta};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, trace};

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("applying k8s resource: `{0}`")]
    ApplyError(String),

    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("building k8s resources: `{0}`")]
    ConfigError(String),

    #[error("building health checkers: `{0}`")]
    HealthError(#[from] HealthCheckerError),
}

/// CRSupervisor - Supervises Kubernetes resources.
/// To be considered:
/// - Uses shared k8s client via Arc; consider design implications about sharing client through all the supervisors.
pub struct CRSupervisor {
    agent_id: AgentID,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: runtime_config::K8s,
}

impl CRSupervisor {
    pub fn new(
        agent_id: AgentID,
        k8s_client: Arc<SyncK8sClient>,
        k8s_config: runtime_config::K8s,
    ) -> Self {
        Self {
            agent_id,
            k8s_client,
            k8s_config,
        }
    }

    pub fn apply(&self) -> Result<Vec<DynamicObject>, SupervisorError> {
        let resources = self.build_dynamic_objects()?;
        debug!("applying k8s objects, if changed, for {}", self.agent_id);
        for res in resources.iter() {
            trace!("K8s object: {:?}", res);
            self.k8s_client.apply_dynamic_object_if_changed(res)?;
        }
        info!(
            "{} K8sSupervisor started and K8s objects applied",
            self.agent_id
        );
        Ok(resources)
    }

    fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorError> {
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

        let metadata = ObjectMeta {
            name: Some(k8s_obj.metadata.name.clone()),
            namespace: Some(self.k8s_client.default_namespace().to_string()),
            labels: Some(labels.get()),
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

    pub fn start_health_check(
        &self,
        health_publisher: EventPublisher<SubAgentInternalEvent>,
        resources: Vec<DynamicObject>,
    ) -> Result<Option<EventPublisher<()>>, SupervisorError> {
        if let Some(health_config) = self.k8s_config.health.clone() {
            let (stop_health_publisher, stop_health_consumer) = pub_sub();
            let k8s_health_checker =
                SubAgentHealthChecker::try_new(self.k8s_client.clone(), resources)?;

            spawn_health_checker(
                self.agent_id.clone(),
                k8s_health_checker,
                stop_health_consumer,
                health_publisher,
                health_config.interval,
            );
            return Ok(Some(stop_health_publisher));
        }

        debug!(%self.agent_id, "health checks are disabled for this agent");
        Ok(None)
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::agent_type::runtime_config::K8sObject;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::{agent_type::runtime_config::K8sObjectMeta, k8s::client::MockSyncK8sClient};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::serde_json;
    use kube::core::TypeMeta;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};

    const TEST_API_VERSION: &str = "test/v1";
    const TEST_KIND: &str = "test";
    const TEST_NAMESPACE: &str = "default";
    const TEST_NAME: &str = "test-name";

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

    #[test]
    fn test_supervisor_apply() {
        let mut mock_k8s_client = MockSyncK8sClient::default();

        let agent_id = AgentID::new("test").unwrap();

        let mut labels = Labels::new(&agent_id);
        labels.append_extra_labels(&k8s_object().metadata.labels);

        let expected = DynamicObject {
            types: Some(TypeMeta {
                api_version: TEST_API_VERSION.to_string(),
                kind: TEST_KIND.to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(TEST_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(labels.get()),
                ..Default::default()
            },
            data: json!({}),
        };
        mock_k8s_client
            .expect_default_namespace()
            .return_const(TEST_NAMESPACE.to_string());

        mock_k8s_client
            .expect_apply_dynamic_object_if_changed()
            .times(2)
            .withf(move |dyn_object| expected.eq(dyn_object))
            .returning(|_| Ok(()));

        let supervisor = CRSupervisor::new(
            agent_id,
            Arc::new(mock_k8s_client),
            runtime_config::K8s {
                objects: HashMap::from([
                    ("mock_cr1".to_string(), k8s_object()),
                    ("mock_cr2".to_string(), k8s_object()),
                ]),
                health: None,
            },
        );

        supervisor.apply().unwrap();
    }
}
