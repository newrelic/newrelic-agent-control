use crate::k8s::executor::{K8sExecutor, K8sExecutorError};
use crate::super_agent::instance_id_getter::{
    DataStored, K8sIdentifiers, Metadata, OnHostIdentifiers,
};

#[feature(async_fn_in_trait)]
pub trait InstanceIDStorer {
    type Identifier: Metadata;
    async fn set(
        &self,
        id: &str,
        data: &DataStored<Self::Identifier>,
    ) -> Result<(), PersisterError>;
    async fn get(&self, id: &str) -> Result<DataStored<Self::Identifier>, PersisterError>;
}

///
/// K8s Implementation
///

pub struct K8sStorer {
    pub configmap_name: String,
    pub k8s_executor: K8sExecutor,
}

#[derive(thiserror::Error, Debug)]
pub enum PersisterError {
    #[error("failed to persist on k8s {0}")]
    FailedToPersistK8s(#[from] K8sExecutorError),

    #[error("failed to persist onHost")]
    FailedToPersistOnHost(),

    #[error("failed to parse yaml: {0}")]
    FailedToPasrseYaml(String),

    #[error("failed to persist onHost")]
    FailedIOTockio(#[from] std::io::Error),
}

impl InstanceIDStorer for K8sStorer {
    type Identifier = K8sIdentifiers;

    async fn set(&self, id: &str, ds: &DataStored<Self::Identifier>) -> Result<(), PersisterError> {
        let data = serde_yaml::to_string(&ds)
            .map_err(|e| PersisterError::FailedToPasrseYaml(e.to_string()))?;

        self.k8s_executor
            .set_configmap_key(&self.configmap_name, id, data.as_str())
            .await?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<DataStored<Self::Identifier>, PersisterError> {
        let data = self
            .k8s_executor
            .get_configmap_key(&self.configmap_name, id)
            .await?;
        let ds = serde_yaml::from_str::<DataStored<K8sIdentifiers>>(data.as_str())
            .map_err(|e| PersisterError::FailedToPasrseYaml(e.to_string()))?;

        Ok(ds)
    }
}

///
/// OnHost Implementation
///

#[derive(Default)]
pub struct OnHostStorer {}

impl InstanceIDStorer for OnHostStorer {
    type Identifier = OnHostIdentifiers;

    async fn set(&self, id: &str, ds: &DataStored<Self::Identifier>) -> Result<(), PersisterError> {
        todo!()
    }

    async fn get(&self, id: &str) -> Result<DataStored<Self::Identifier>, PersisterError> {
        todo!()
    }
}
