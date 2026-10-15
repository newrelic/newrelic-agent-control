use serde::Deserialize;
use serde_json::{Value, from_str, from_value};
use std::collections::HashMap;
use std::str::FromStr;

/// Agents under test, identified by their Instrumentation Metadata agent type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum AgentKind {
    #[serde(rename = "NRInfra")]
    InfraAgent,
    #[serde(rename = "NRDOT")]
    Nrdot,
    #[serde(rename = "NRIRedis")]
    Redis,
    #[serde(rename = "NRINginx")]
    Nginx,
    #[serde(rename = "NRIApache")]
    Apache,
    #[serde(rename = "NRIMemcached")]
    Memcached,
    #[serde(rename = "NReBPFAgent")]
    EbpfAgent,
    #[serde(rename = "NRIFlex")]
    Flex,
    #[serde(rename = "NRIMysql")]
    Mysql,
    #[serde(rename = "NRIPostgresql")]
    Postgresql,
}

impl AgentKind {
    /// Agent type name, without version.
    pub fn agent_type_name(self) -> &'static str {
        match self {
            Self::InfraAgent => "newrelic/com.newrelic.infrastructure",
            Self::Nrdot => "newrelic/com.newrelic.opentelemetry.collector",
            Self::Redis => "newrelic/com.newrelic.infrastructure.nri_redis",
            Self::Nginx => "newrelic/com.newrelic.infrastructure.nri_nginx",
            Self::Apache => "newrelic/com.newrelic.infrastructure.nri_apache",
            Self::Memcached => "newrelic/com.newrelic.infrastructure.nri_memcached",
            Self::EbpfAgent => "newrelic/com.newrelic.ebpf",
            Self::Flex => "newrelic/com.newrelic.infrastructure.nri_flex",
            Self::Mysql => "newrelic/com.newrelic.infrastructure.nri_mysql",
            Self::Postgresql => "newrelic/com.newrelic.infrastructure.nri_postgresql",
        }
    }

    /// Sub-agent id, as assigned by Fleet Control.
    pub fn sub_agent_id(self) -> &'static str {
        match self {
            Self::InfraAgent => "nr-infra",
            Self::Nrdot => "nrdot",
            Self::Redis => "nr-redis",
            Self::Nginx => "nr-nginx",
            Self::Apache => "nr-apache",
            Self::Memcached => "nr-memcached",
            Self::EbpfAgent => "nr-ebpf",
            Self::Flex => "nr-flex",
            Self::Mysql => "nr-mysql",
            Self::Postgresql => "nr-postgresql",
        }
    }

    /// Name of the infra-agent integration, for the agents that are one.
    pub fn integration_name(self) -> Option<&'static str> {
        match self {
            Self::Redis => Some("nri-redis"),
            Self::Nginx => Some("nri-nginx"),
            Self::Apache => Some("nri-apache"),
            Self::Memcached => Some("nri-memcached"),
            Self::Flex => Some("nri-flex"),
            Self::Mysql => Some("nri-mysql"),
            Self::Postgresql => Some("nri-postgresql"),
            Self::InfraAgent | Self::Nrdot | Self::EbpfAgent => None,
        }
    }
}

/// Versions of an agent resolved by the `agent-versions` action.
#[derive(Debug, Clone)]
pub struct AgentMetadata {
    kind: AgentKind,
    package_version: String,
    agent_type_version: String,
}

impl AgentMetadata {
    pub fn package_version(&self) -> &str {
        &self.package_version
    }

    pub fn agent_type_id(&self) -> String {
        format!(
            "{}:{}",
            self.kind.agent_type_name(),
            self.agent_type_version
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMetadataEntry {
    im_agent_type: String,
    package_version: String,
    agent_type_version: String,
}

/// Metadata of the resolved agents, parsed from the `json` output of the `agent-versions` action.
#[derive(Debug, Clone, Default)]
pub struct AgentMetadataSet(HashMap<AgentKind, AgentMetadata>);

impl AgentMetadataSet {
    pub fn get(&self, agent: AgentKind) -> &AgentMetadata {
        self.0
            .get(&agent)
            .unwrap_or_else(|| panic!("no resolved metadata for {agent:?}"))
    }
}

impl FromStr for AgentMetadataSet {
    type Err = serde_json::Error;

    fn from_str(json: &str) -> Result<Self, Self::Err> {
        let entries: Vec<AgentMetadataEntry> = from_str(json)?;
        Ok(Self(
            entries
                .into_iter()
                .filter_map(|entry| {
                    // The action resolves every IM agent type; only those under test are kept.
                    let kind = from_value(Value::String(entry.im_agent_type)).ok()?;
                    let metadata = AgentMetadata {
                        kind,
                        package_version: entry.package_version,
                        agent_type_version: entry.agent_type_version,
                    };
                    Some((kind, metadata))
                })
                .collect(),
        ))
    }
}
