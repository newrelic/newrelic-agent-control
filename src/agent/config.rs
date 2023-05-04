use serde::Deserialize;
use std::collections::HashMap;
use serde_json::Value;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config {
    pub op_amp: String,
    pub agents: HashMap<String, Value>,
}

pub trait Getter {
    fn get(&self) -> Config;
}

#[cfg(test)]
mod tests {
    #[test]
    fn exploration() {
        assert_eq!(2 + 2, 4);
    }
}

