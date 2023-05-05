use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Config<V: Debug>  {
    pub op_amp: String,
    pub agents: HashMap<String, V>,
}

pub trait Getter<V: Debug> {
    fn get(&self) -> Config<V>;
}
