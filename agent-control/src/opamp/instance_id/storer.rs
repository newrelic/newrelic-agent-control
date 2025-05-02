use crate::{agent_control::agent_id::AgentID, opamp::instance_id::getter::DataStored};

use super::{definition::InstanceIdentifiers, getter::GetterError};

pub trait InstanceIDStorer
where
    GetterError: From<Self::Error>,
{
    type Error;
    type Identifiers: InstanceIdentifiers;

    fn set(
        &self,
        agent_id: &AgentID,
        data: &DataStored<Self::Identifiers>,
    ) -> Result<(), Self::Error>;
    fn get(&self, agent_id: &AgentID)
    -> Result<Option<DataStored<Self::Identifiers>>, Self::Error>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::opamp::instance_id::definition::tests::MockIdentifiers;

    use super::*;
    use mockall::mock;
    use thiserror::Error;

    #[derive(Error, Debug, PartialEq, Clone)]
    #[error("mock getter error")]
    pub struct MockStorerError;

    impl From<MockStorerError> for GetterError {
        fn from(_: MockStorerError) -> Self {
            GetterError::MockGetterError
        }
    }

    mock! {
        pub InstanceIDStorer {}

        impl InstanceIDStorer for InstanceIDStorer {
            type Error = MockStorerError;
            type Identifiers = MockIdentifiers;

            fn set(&self, agent_id: &AgentID, data: &DataStored<MockIdentifiers>) -> Result<(), MockStorerError>;
            fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored<MockIdentifiers>>, MockStorerError>;
        }
    }
}
