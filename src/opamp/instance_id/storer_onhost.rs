use crate::opamp::instance_id::getter::{DataStored, InstanceID};
use crate::opamp::instance_id::storer::InstanceIDStorer;
use ulid::Ulid;

pub struct Storer {}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("generic storer error")]
    Generic,
}

impl Storer {
    pub fn new() -> Self {
        Self {}
    }
}

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
