use crate::k8s::error::K8sError;
use crate::k8s::labels::Labels;
use crate::{
    agent_type_definition::runtime_config::K8sObject, config::super_agent_configs::AgentID,
};
use k8s_openapi::serde_json;
use kube::{
    api::DynamicObject,
    core::{ObjectMeta, TypeMeta},
};

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, trace};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("applying k8s resource {0}")]
    ApplyError(String),

    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("applying k8s resource {0}")]
    ConfigError(String),
}

/// CRSupervisor - Supervises Kubernetes resources.
/// To be considered:
/// - Uses shared k8s client via Arc; consider design implications about sharing client through all the supervisors.
pub struct CRSupervisor {
    agent_id: AgentID,
    k8s_client: Arc<SyncK8sClient>,
    k8s_objects: HashMap<String, K8sObject>,
}

impl CRSupervisor {
    pub fn new(
        agent_id: AgentID,
        k8s_client: Arc<SyncK8sClient>,
        k8s_objects: HashMap<String, K8sObject>,
    ) -> Self {
        Self {
            agent_id,
            k8s_client,
            k8s_objects,
        }
    }

    pub fn apply(&self) -> Result<(), SupervisorError> {
        let resources = self.build_dynamic_objects()?;
        for res in resources {
            debug!("Applying k8s object for {}", self.agent_id,);
            trace!("K8s object: {:?}", res);
            self.k8s_client.apply_dynamic_object_if_changed(&res)?;
        }
        info!(
            "{} K8sSupervisor started and K8s objects applied",
            self.agent_id
        );
        Ok(())
    }

    fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorError> {
        self.k8s_objects
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
        if let Some(metadata) = &k8s_obj.metadata {
            // Merge default labels with the ones coming from the config with default labels taking precedence.
            labels.append_extra_labels(&metadata.labels);
        }

        let metadata = ObjectMeta {
            name: Some(self.agent_id.to_string()),
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
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::k8s::labels::AGENT_ID_LABEL_KEY;
    use crate::{
        agent_type_definition::runtime_config::K8sObjectMeta, k8s::client::MockSyncK8sClient,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::serde_json;
    use kube::core::TypeMeta;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};

    const TEST_API_VERSION: &str = "test/v1";
    const TEST_KIND: &str = "test";
    const NAMESPACE: &str = "default";

    fn k8s_object() -> K8sObject {
        K8sObject {
            api_version: TEST_API_VERSION.to_string(),
            kind: TEST_KIND.to_string(),
            metadata: Some(K8sObjectMeta {
                labels: BTreeMap::from([
                    ("custom-label".to_string(), "values".to_string()),
                    (
                        AGENT_ID_LABEL_KEY.to_string(),
                        "to be overwritten".to_string(),
                    ),
                ]),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_supervisor_apply() {
        let mut mock_k8s_client = MockSyncK8sClient::default();

        let agent_id = AgentID::new("test").unwrap();

        let mut labels = Labels::new(&agent_id);
        labels.append_extra_labels(&k8s_object().metadata.unwrap().labels);

        let expected = DynamicObject {
            types: Some(TypeMeta {
                api_version: TEST_API_VERSION.to_string(),
                kind: TEST_KIND.to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(agent_id.get()),
                namespace: Some(NAMESPACE.to_string()),
                labels: Some(labels.get()),
                ..Default::default()
            },
            data: json!({}),
        };
        mock_k8s_client
            .expect_default_namespace()
            .return_const(NAMESPACE.to_string());

        mock_k8s_client
            .expect_apply_dynamic_object_if_changed()
            .times(2)
            .withf(move |dyn_object| expected.eq(dyn_object))
            .returning(|_| Ok(()));

        let supervisor = CRSupervisor::new(
            agent_id,
            Arc::new(mock_k8s_client),
            HashMap::from([
                ("mock_cr1".to_string(), k8s_object()),
                ("mock_cr2".to_string(), k8s_object()),
            ]),
        );

        supervisor.apply().unwrap();
    }
}
