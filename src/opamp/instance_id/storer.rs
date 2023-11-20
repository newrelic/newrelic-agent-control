use crate::opamp::instance_id::getter::DataStored;

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("generic storer error")]
    Generic,
}

pub trait InstanceIDStorer {
    fn set(&self, agent_fqdn: &str, data: &DataStored) -> Result<(), StorerError>;
    fn get(&self, agent_fqdn: &str) -> Result<Option<DataStored>, StorerError>;
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub InstanceIDStorerMock {}

        impl InstanceIDStorer for InstanceIDStorerMock {
            fn set(&self, agent_fqdn: &str, data: &DataStored) -> Result<(), StorerError>;
            fn get(&self, agent_fqdn: &str) -> Result<Option<DataStored>, StorerError>;
        }
    }
}
