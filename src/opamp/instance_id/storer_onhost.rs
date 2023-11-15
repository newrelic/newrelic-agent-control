use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use ulid::Ulid;

pub struct Storer {}

impl InstanceIDStorer for Storer {
    fn set(&self, _agent_fqdn: &str, _ds: &DataStored) -> Result<(), StorerError> {
        // TODO
        Ok(())
    }

    fn get(&self, _agent_fqdn: &str) -> Result<Option<DataStored>, StorerError> {
        // TODO
        Ok(Some(DataStored {
            ulid: Ulid::new(),
            identifiers: Default::default(),
        }))
    }
}
