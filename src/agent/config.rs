use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub(crate) struct Config<V: Debug>  {
    pub(crate) op_amp: String,
    pub(crate) agents: HashMap<String, V>,
}

pub(crate) trait Getter<V: Debug> {
    fn get(&self) -> Config<V>;
}
