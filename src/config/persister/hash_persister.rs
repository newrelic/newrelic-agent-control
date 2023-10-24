use crate::config::persister::config_persister::PersistError;

pub trait HashPersister {
    fn persist(&self, hash: String) -> Result<(), PersistError>;
}

#[cfg(test)]
pub mod test {}
