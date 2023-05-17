pub(crate) mod config;

use std::fmt::{Debug, Display, Formatter, Result as FmtResult};
use std::marker::PhantomData;

use crate::agent::config::Getter;

pub struct AgentError;

impl Display for AgentError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "invalid first item to double")
    }
}

/// The Agent Struct that injects a config getter that implements
/// the config::Getter trait and uses the V value serializer
pub struct Agent<G: Getter<V>, V: Debug> {
    conf_getter: G,
    phantom: PhantomData<V>,
}

impl<G: Getter<V>, V: Debug> Agent<G, V> {
    pub fn new(getter: G) -> Self {
        Self {
            conf_getter: getter,
            phantom: PhantomData,
        }
    }

    /// The start function calls the config getter to print the configuration.
    pub fn start(&self) -> Result<(), AgentError> {
        let parsed_config = self.conf_getter.get();
        println!("{:?}", parsed_config);
        Ok(())
    }
}
