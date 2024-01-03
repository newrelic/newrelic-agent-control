use crate::config::super_agent_configs::AgentID;
use crate::k8s;
use crate::k8s::labels::Labels;
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use std::sync::Arc;
use tracing::debug;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

pub struct Storer {
    k8s_executor: Arc<K8sExecutor>,
    configmap_prefix: String,
}

pub const CM_KEY: &str = "ulid";
const CM_PREFIX: &str = "ulid-data";

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("failed to persist on k8s {0}")]
    FailedToPersistK8s(#[from] k8s::Error),

    #[error("failed to persist onHost")]
    FailedToPersistOnHost(),

    #[error("failed to parse yaml: {0}")]
    FailedToPasrseYaml(#[from] serde_yaml::Error),

    #[error("generic storer error")]
    Generic,
}

impl InstanceIDStorer for Storer {
    fn set(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        crate::runtime::runtime().block_on(self.async_set(agent_id, ds))
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        crate::runtime::runtime().block_on(self.async_get(agent_id))
    }
}

impl Storer {
    pub fn new(k8s_executor: Arc<K8sExecutor>) -> Self {
        Self {
            k8s_executor,
            configmap_prefix: CM_PREFIX.to_string(),
        }
    }

    async fn async_set(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        let cm_name: String = build_cm_name(&self.configmap_prefix, agent_id);

        let data = serde_yaml::to_string(&ds)?;

        debug!("storer: setting ULID of agent_id:{}", agent_id);
        self.k8s_executor
            .set_configmap_key(&cm_name, Labels::new(agent_id).get(), CM_KEY, data.as_str())
            .await?;

        Ok(())
    }

    async fn async_get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        let cm_name: String = build_cm_name(&self.configmap_prefix, agent_id);

        debug!("storer: getting ULID of agent_id:{}", agent_id);

        let data_res = self
            .k8s_executor
            .get_configmap_key(&cm_name, CM_KEY)
            .await?;
        match data_res {
            Some(data) => {
                let ds = serde_yaml::from_str::<DataStored>(data.as_str())?;

                Ok(Some(ds))
            }
            None => Ok(None),
        }
    }
}

fn build_cm_name(prefix: &String, agent_id: &str) -> String {
    let mut cm_name = prefix.to_owned();
    cm_name.push('-');
    cm_name.push_str(agent_id);

    cm_name.to_owned()
}

#[cfg(test)]
pub mod test {
    use super::{Storer, CM_KEY};
    use crate::config::super_agent_configs::AgentID;
    use crate::k8s::error::K8sError;
    use crate::k8s::executor::MockK8sExecutor;
    use crate::k8s::labels::Labels;
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::storer::InstanceIDStorer;
    use crate::opamp::instance_id::InstanceID;
    use mockall::predicate;
    use std::sync::Arc;

    const AGENT_NAME: &str = "agent1";
    const DATA_STORED: &str = "ulid: 01HFW1YZKYWHTGC0WMWPNR4P4K
identifiers:
  cluster_name: ''
";
    const ULID: &str = "01HFW1YZKYWHTGC0WMWPNR4P4K";
    const EXPECTED_CM_NAME: &str = "ulid-data-agent1";

    #[test]
    fn test_input_parameters_dependencies() {
        // In this tests we are checking that the parameters are passed as expected and that cm names are built in the proper way
        // The output of the commands are checked in following tests.
        let mut m = MockK8sExecutor::default();
        m.expect_get_configmap_key()
            .once()
            .with(
                predicate::function(|name| name == EXPECTED_CM_NAME),
                predicate::function(|key| key == CM_KEY),
            )
            .returning(move |_, _| Err(K8sError::CMMalformed()));
        m.expect_set_configmap_key()
            .once()
            .with(
                predicate::function(|name| name == EXPECTED_CM_NAME),
                predicate::function(|key| {
                    key == &Labels::new(&AgentID::new(AGENT_NAME).unwrap()).get()
                }),
                predicate::function(|key| key == CM_KEY),
                predicate::function(|ds| ds == DATA_STORED),
            )
            .returning(move |_, _, _, _| Err(K8sError::CMMalformed()));
        let s = Storer::new(Arc::new(m));
        let _ = s.get(&AgentID::new(AGENT_NAME).unwrap());
        let _ = s.set(
            &AgentID::new(AGENT_NAME).unwrap(),
            &DataStored {
                ulid: InstanceID::new(ULID.to_string()),
                identifiers: Default::default(),
            },
        );
    }

    #[test]
    fn test_get_error() {
        let mut m = MockK8sExecutor::default();
        m.expect_get_configmap_key()
            .once()
            .returning(move |_, _| Err(K8sError::CMMalformed()));
        let s = Storer::new(Arc::new(m));

        let id = s.get(&AgentID::new(AGENT_NAME).unwrap());
        assert!(id.is_err())
    }

    #[test]
    fn test_get_not_found() {
        let mut m = MockK8sExecutor::default();
        m.expect_get_configmap_key()
            .once()
            .returning(move |_, _| Ok(None));
        let s = Storer::new(Arc::new(m));

        let id = s.get(&AgentID::new(AGENT_NAME).unwrap());
        assert!(id.is_ok());
        assert!(id.unwrap().is_none());
    }

    #[test]
    fn test_get_found_data() {
        let mut m = MockK8sExecutor::default();
        m.expect_get_configmap_key()
            .once()
            .returning(move |_, _| Ok(Some(DATA_STORED.to_string())));
        let s = Storer::new(Arc::new(m));

        let id = s.get(&AgentID::new(AGENT_NAME).unwrap());
        assert!(id.is_ok());
        let id_un = id.unwrap();
        assert!(id_un.is_some());
        let td = id_un.unwrap();
        assert_eq!(td.ulid, InstanceID::new(ULID.to_string()))
    }

    #[test]
    fn test_set_error() {
        let mut m = MockK8sExecutor::default();
        m.expect_set_configmap_key()
            .once()
            .returning(move |_, _, _, _| Err(K8sError::CMMalformed()));
        let s = Storer::new(Arc::new(m));

        let id = s.set(
            &AgentID::new(AGENT_NAME).unwrap(),
            &DataStored {
                ulid: Default::default(),
                identifiers: Default::default(),
            },
        );
        assert!(id.is_err())
    }

    #[test]
    fn test_set_succeeded() {
        let mut m = MockK8sExecutor::default();
        m.expect_set_configmap_key()
            .once()
            .returning(move |_, _, _, _| Ok(()));
        let s = Storer::new(Arc::new(m));
        let id = s.set(
            &AgentID::new(AGENT_NAME).unwrap(),
            &DataStored {
                ulid: Default::default(),
                identifiers: Default::default(),
            },
        );
        assert!(id.is_ok())
    }
}
