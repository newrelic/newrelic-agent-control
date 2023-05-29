use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;

/// The Config for the meta-agent and the managers
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Config<V: Debug> {
    // pub(crate) op_amp: String,
    pub(crate) agents: HashMap<String, V>,
}

impl<V: Debug> Config<V> {
    pub fn agents(&self) -> &HashMap<String, V> {
        &self.agents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de;
    use serde::Deserialize;
    use serde_json::Value;

    #[derive(Debug, PartialEq)]
    pub enum CustomTypeTest {
        A,
        B,
    }

    impl<'de> de::Deserialize<'de> for CustomTypeTest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            if s == "type-a" {
                return Ok(CustomTypeTest::A);
            }
            Ok(CustomTypeTest::B)
        }
    }

    #[test]
    fn test_deserialize_agent_config() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct InfraAgent {
            uuid_dir: String,
            value: i64,
            kind: CustomTypeTest,
        }

        let yaml_cfg = r#"{
            "op_amp": "John Doe",
            "agents": {
                "nr_otel_collector/gateway": {},
                "nr_infra_agent": {
                    "uuid_dir": "/bin/sudoo",
                    "value": 1,
                    "kind": "type-a"
                }
            }
        }"#;

        let cfg: Config<Value> = serde_json::from_str(yaml_cfg).unwrap();

        let infra_agent = cfg.agents.get("nr_infra_agent").unwrap();
        let agent: InfraAgent = serde_json::from_value(infra_agent.clone()).unwrap();

        let expected = InfraAgent {
            uuid_dir: "/bin/sudoo".to_string(),
            value: 1 as i64,
            kind: CustomTypeTest::A,
        };

        assert_eq!(agent, expected);
    }
}
