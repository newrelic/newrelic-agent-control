use crate::k8s::error::K8sError;
use crate::k8s::executor::K8sExecutor;
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;

pub struct Storer {
    pub k8s_executor: K8sExecutor,
    pub configmap_prefix: String,
}

const CM_KEY: &str = "ulid-data";

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("failed to persist on k8s {0}")]
    FailedToPersistK8s(#[from] K8sError),

    #[error("failed to persist onHost")]
    FailedToPersistOnHost(),

    #[error("failed to parse yaml: {0}")]
    FailedToPasrseYaml(String),

    #[error("generic storer error")]
    Generic,
}

impl InstanceIDStorer for Storer {
    fn set(&self, agent_id: &str, ds: &DataStored) -> Result<(), StorerError> {
        futures::executor::block_on(self.async_set(agent_id, ds))
    }

    fn get(&self, agent_id: &str) -> Result<Option<DataStored>, StorerError> {
        futures::executor::block_on(self.async_get(agent_id))
    }
}

impl Storer {
    async fn async_set(&self, agent_id: &str, ds: &DataStored) -> Result<(), StorerError> {
        let cm_name: String = build_cm_name(&self.configmap_prefix, agent_id);

        let data = serde_yaml::to_string(&ds)
            .map_err(|e| StorerError::FailedToPasrseYaml(e.to_string()))?;

        self.k8s_executor
            .set_configmap_key(&cm_name, CM_KEY, data.as_str())
            .await?;

        Ok(())
    }

    async fn async_get(&self, agent_id: &str) -> Result<Option<DataStored>, StorerError> {
        let cm_name: String = build_cm_name(&self.configmap_prefix, agent_id);

        let data_res = self
            .k8s_executor
            .get_configmap_key(&cm_name, CM_KEY)
            .await?;
        match data_res {
            Some(data) => {
                let ds = serde_yaml::from_str::<DataStored>(data.as_str())
                    .map_err(|e| StorerError::FailedToPasrseYaml(e.to_string()))?;

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
