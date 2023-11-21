use crate::opamp::instance_id::getter::{DataStored, InstanceID};
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use ulid::Ulid;

pub struct Storer {}

impl InstanceIDStorer for Storer {
    fn set(&self, _agent_id: &str, _ds: &DataStored) -> Result<(), StorerError> {
        // TODO
        Ok(())
    }

    fn get(&self, _agent_id: &str) -> Result<Option<DataStored>, StorerError> {
        // TODO
        Ok(Some(DataStored {
            ulid: InstanceID::new(Ulid::new().to_string()),
            identifiers: Default::default(),
        }))
    }
}
