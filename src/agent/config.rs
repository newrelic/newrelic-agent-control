use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Config<V: Debug> {
    pub(crate) op_amp: String,
    pub(crate) agents: HashMap<String, V>,
}

pub trait Getter<V: Debug> {
    fn get(&self) -> Config<V>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::Value;

    #[test]
    fn test_deserialize_agent_config() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct InfraAgent {
            uuid_dir: String,
            value: i64,
        }

        let yaml_cfg = r#"{
            "op_amp": "John Doe",
            "agents": {
                "nr_otel_collector/gateway": {},
                "nr_infra_agent": {
                    "uuid_dir": "/bin/sudoo",
                    "value": 1
                }
            }
    }"#;

        let cfg: Config<Value> = serde_json::from_str(yaml_cfg).unwrap();

        let infra_agent = cfg.agents.get("nr_infra_agent").unwrap();
        let agent: InfraAgent = serde_json::from_value(infra_agent.clone()).unwrap();

        let expected = InfraAgent {
            uuid_dir: "/bin/sudoo".to_string(),
            value: 1 as i64,
        };

        assert_eq!(agent, expected);
    }
}
