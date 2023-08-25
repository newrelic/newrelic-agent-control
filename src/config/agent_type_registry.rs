use std::collections::HashMap;

use thiserror::Error;

use crate::config::agent_type::{Agent, Deployment, Executable, Meta, OnHost};


#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
pub trait AgentRepository {
    // get returns an Agent type given a definition.
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError>;

    // stores a given Agent type.
    fn store(&mut self, agent: Agent) -> Result<(), AgentRepositoryError>;
}

pub struct LocalRepository(HashMap<String, Agent>);

impl AgentRepository for LocalRepository {
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError> {
        self.0.get(name).ok_or(AgentRepositoryError::NotFound)
    }

    fn store(&mut self, agent: Agent) -> Result<(), AgentRepositoryError> {
        self.0.insert(agent.name.clone(), agent);
        Ok(())
    }
}

impl LocalRepository {
    pub(crate) fn new() -> Self {
        const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra";
        const NEWRELIC_INFRA_ARGS: [&str; 2] = [
            "--config",
            "/etc/newrelic-infra.yml"
        ];

        const NRDOT_PATH: &str = "/usr/bin/nr-otel-collector";
        const NRDOT_ARGS: [&str; 3] = [
            "--config",
            "/etc/nr-otel-collector/config.yaml",
            "--feature-gates=-pkg.translator.prometheus.NormalizeName",
        ];
        LocalRepository(
            HashMap::from([
                ("nr_otel_collector".to_string(), Agent{
                    name: "nr_otel_collector".to_string(),
                    namespace: "".to_string(),
                    version: "".to_string(),
                    spec: Default::default(),
                    meta: Meta{
                        deployment: Deployment{
                            on_host: Option::from(
                                OnHost { executables: vec![
                                    Executable{
                                        path: NRDOT_PATH.to_string(),
                                        args: NRDOT_ARGS.concat(),
                                        env: "".to_string(),
                                    }
                                ]}
                            )
                        }
                    },
                }),
                ("nr_infra_agent".to_string(), Agent{
                    name: "nr_infra_agent".to_string(),
                    namespace: "".to_string(),
                    version: "".to_string(),
                    spec: Default::default(),
                    meta: Meta{
                        deployment: Deployment{
                            on_host: Option::from(
                                OnHost { executables: vec![
                                    Executable{
                                        path: NEWRELIC_INFRA_PATH.to_string(),
                                        args: NRDOT_ARGS.concat(),
                                        env: "".to_string(),
                                    }
                                ]}
                            )
                        }
                    },
                })
            ])
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::config::agent_type::tests::AGENT_GIVEN_YAML;

    use super::*;

    fn retrive_agent<R>(reader: R) -> super::Agent
    where
        R: std::io::Read,
    {
        let raw_agent: crate::config::agent_type::RawAgent =
            serde_yaml::from_reader(reader).unwrap();

        super::Agent::from(raw_agent)
    }

    #[test]
    fn add_multiple_agents() {
        let mut repository = LocalRepository::new();

        assert!(repository
            .store(retrive_agent(AGENT_GIVEN_YAML.as_bytes()))
            .is_ok());

        assert_eq!(repository.get(&"nrdot".to_string()).unwrap().name, "nrdot");

        let invalid_lookup = repository.get(&"not_an_agent".to_string());
        assert!(invalid_lookup.is_err());

        assert_eq!(
            invalid_lookup.unwrap_err().to_string(),
            "agent not found".to_string()
        )
    }
}
