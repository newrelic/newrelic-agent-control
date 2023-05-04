pub mod config;

use std::error::Error;
use crate::agent::config::Getter;

pub struct Agent<G: Getter> {
    conf_getter: G
}

impl<G: Getter> Agent<G> {
    pub fn new(getter: G) -> Self {
        Self {
            conf_getter: getter,
        }
    }

    pub fn start(&self) -> Result<(), Box<dyn Error>> {
        let parsed_config = self.conf_getter.get();
        println!("{:?}", parsed_config);
        Ok(())
    }
}
