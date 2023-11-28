use crate::config::super_agent_configs::AgentID;
use crate::opamp::instance_id::getter::{DataStored, InstanceID};
use crate::opamp::instance_id::storer::InstanceIDStorer;
use crate::opamp::instance_id::Identifiers;
use ulid::Ulid;

#[derive(Default)]
pub struct Storer {}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("generic storer error")]
    Generic,
}

impl InstanceIDStorer for Storer {
    fn set(&self, _agent_id: &AgentID, _ds: &DataStored) -> Result<(), StorerError> {
        // TODO
        Ok(())
    }

    fn get(&self, _agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        // TODO
        Ok(Some(DataStored {
            ulid: InstanceID::new(Ulid::new().to_string()),
            identifiers: Identifiers::default(),
        }))
    }
}
