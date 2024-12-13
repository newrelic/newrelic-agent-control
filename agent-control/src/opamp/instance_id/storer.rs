use super::StorerError;
use crate::{agent_control::config::AgentID, opamp::instance_id::getter::DataStored};

pub trait InstanceIDStorer {
    fn set(&self, agent_id: &AgentID, data: &DataStored) -> Result<(), StorerError>;
    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub InstanceIDStorerMock {}

        impl InstanceIDStorer for InstanceIDStorerMock {
            fn set(&self, agent_id: &AgentID, data: &DataStored) -> Result<(), StorerError>;
            fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError>;
        }
    }
}
