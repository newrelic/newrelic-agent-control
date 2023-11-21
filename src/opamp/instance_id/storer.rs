use super::StorerError;
use crate::opamp::instance_id::getter::DataStored;

pub trait InstanceIDStorer {
    fn set(&self, agent_id: &str, data: &DataStored) -> Result<(), StorerError>;
    fn get(&self, agent_id: &str) -> Result<Option<DataStored>, StorerError>;
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub InstanceIDStorerMock {}

        impl InstanceIDStorer for InstanceIDStorerMock {
            fn set(&self, agent_id: &str, data: &DataStored) -> Result<(), StorerError>;
            fn get(&self, agent_id: &str) -> Result<Option<DataStored>, StorerError>;
        }
    }
}
