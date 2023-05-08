pub mod config;

use std::error::Error;
use std::marker::PhantomData;
use std::fmt::Debug;

use crate::agent::config::Getter;

pub(crate) struct Agent<G: Getter<V>, V:Debug> {
    conf_getter: G,
    phantom: PhantomData<V>
}

impl<G: Getter<V>, V:Debug> Agent<G, V> {
    pub(crate) fn new(getter: G) -> Self {
        Self {
            conf_getter: getter,
            phantom: PhantomData
        }
    }

    pub(crate) fn start(&self) -> Result<(), Box<dyn Error>> {
        let parsed_config = self.conf_getter.get();
        println!("{:?}", parsed_config);
        Ok(())
    }
}
