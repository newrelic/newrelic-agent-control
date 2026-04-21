use crate::agent_control::agent_id::AgentID;

use crate::agent_type::agent_type_id::AgentTypeID;
use crate::checkers::health::health_checker::Health;
use crate::checkers::health::with_start_time::HealthWithStartTime;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::identity::AgentIdentity;
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType};
use serde::Serialize;

use std::collections::HashMap;
use std::time::SystemTime;
use url::Url;

const IDENTIFYING_ATTRIBUTES_PREFIX: &str = "identifying";
const NON_IDENTIFYING_ATTRIBUTES_PREFIX: &str = "non-identifying";

/// Dynamic fields describing the agent; includes attributes like agent_version, instance_uid, ...
pub(super) type AgentAttributes = HashMap<String, String>;

/// Encodes the provided [AgentDescription] as key-val. If there is an overlap in non-identifying
/// and identifying attributes, the identifying attributes take precedence.
pub(super) fn build_agent_attributes(agent_description: AgentDescription) -> AgentAttributes {
    let mut attributes = encode_attributes(
        agent_description.non_identifying_attributes,
        NON_IDENTIFYING_ATTRIBUTES_PREFIX,
    );
    attributes.extend(encode_attributes(
        agent_description.identifying_attributes,
        IDENTIFYING_ATTRIBUTES_PREFIX,
    ));
    attributes
}

/// Helper to encode attributes from `agent_description` as key-val.
/// The keys are prefixed by `{prefix}/`. Example `"agent.id"` with
fn encode_attributes(
    attributes: HashMap<String, DescriptionValueType>,
    prefix: &str,
) -> HashMap<String, String> {
    attributes
        .into_iter()
        .map(|(k, v)| (format!("{prefix}/{k}"), encode_description_value(v)))
        .collect()
}

/// Helper to encode a description value as string. Bytes are encoded as lowercase hex characters.
fn encode_description_value(v: DescriptionValueType) -> String {
    match v {
        DescriptionValueType::String(v) => v,
        DescriptionValueType::Int(v) => v.to_string(),
        DescriptionValueType::Bool(v) => v.to_string(),
        DescriptionValueType::Float(v) => v.to_string(),
        DescriptionValueType::Bytes(v) => v
            .iter()
            .fold(String::new(), |acc, b| format!("{acc}{b:02x}")),
    }
}

/// Agent Control status and health information.
/// This information will be shown when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "agent_control": {
///     "healthy": true,
///     "last_error": "",
///     "status": ""
///   },
/// }
/// ```
#[derive(Debug, Serialize, PartialEq, Default, Clone)]
pub struct AgentControlStatus {
    healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
    #[serde(skip_serializing_if = "AgentAttributes::is_empty")]
    pub(super) attributes: AgentAttributes,
}

impl AgentControlStatus {
    pub fn set_health(&mut self, health: HealthWithStartTime) {
        match Health::from(health) {
            Health::Healthy(healthy) => {
                self.healthy = true;
                self.last_error = None;
                self.status = healthy.status().to_string();
            }
            Health::Unhealthy(unhealthy) => {
                self.healthy = false;
                self.last_error = unhealthy.last_error().to_string().into();
                self.status = unhealthy.status().to_string();
            }
        }
    }
}

/// OpAMP Connection health information.
/// This information will be shown when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "fleet": {
///     "enabled": true,
///     "endpoint": "https://example.com/opamp/v1",
///     "reachable": true,
///     "error_code": 403, // present only if reachable == false
///     "error_message": "this is an error message", // present only if reachable == false
///   }
/// }
/// ```
#[derive(Debug, Serialize, PartialEq, Default, Clone)]
pub struct OpAMPStatus {
    enabled: bool,
    endpoint: Option<Url>,
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<LastErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<LastErrorMessage>,
}

impl OpAMPStatus {
    pub(super) fn reachable(&mut self) {
        self.reachable = true;
        self.error_code = None;
        self.error_message = None;
    }

    pub(super) fn unreachable(
        &mut self,
        error_code: Option<LastErrorCode>,
        error_message: LastErrorMessage,
    ) {
        self.reachable = false;
        self.error_code = error_code;
        self.error_message = Some(error_message);
    }
}

/// Sub Agent status and health information.
/// This information is displayed when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "agents": [
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "health_info": {
///         "healthy": true,
///         "last_error": null,
///         "status": "",
///         "start_time_unix_nano": 0,
///         "status_time_unix_nano": 0
///       },
///       "agent_start_time_unix_nano": 0
///     },
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "health_info": {
///         "healthy": false,
///         "last_error": "The sub-agent exceeded the number of retries defined in its restart policy.",
///         "status": "[xx/xx/xx xx:xx:xx.xxxx] debug: could not read config at /etc/newrelic-infra.yml",
///         "start_time_unix_nano": 0,
///         "status_time_unix_nano": 0
///       },
///       "agent_start_time_unix_nano": 0
///     }
///   ]
/// }
/// ```
///
/// Fields:
/// - `agent_id`: The unique identifier of the Sub Agent.
/// - `agent_type`: The type of the Sub Agent, represented as a fully qualified name (FQN).
/// - `agent_start_time_unix_nano`: A `u64` representing the start time of the Sub Agent in nanoseconds since the Unix epoch.
/// - `health_info`: A `HealthInfo` struct containing the health-related information of the Sub Agent.
/// - `attributes`: A map of dynamic agent attributes such as version, instance_uid, ...
#[derive(Debug, Serialize, PartialEq, Clone)]
pub(super) struct SubAgentStatus {
    agent_id: AgentID,
    #[serde(serialize_with = "AgentTypeID::serialize_fqn")]
    agent_type: AgentTypeID,
    agent_start_time_unix_nano: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    health_info: Option<HealthInfo>,
    #[serde(skip_serializing_if = "AgentAttributes::is_empty")]
    pub(super) attributes: AgentAttributes,
}

/// Health-related information of a Sub Agent.
/// This struct is used to represent the health status of a Sub Agent
/// and is displayed when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "healthy": true,
///   "last_error": null,
///   "status": "Running",
///   "start_time_unix_nano": 1672531200000000000,
///   "status_time_unix_nano": 1672531205000000000
/// }
/// ```
///
/// Fields:
/// - `healthy`: A boolean indicating whether the Sub Agent is healthy.
/// - `last_error`: An optional string containing the last error message, if any.
/// - `status`: A string representing the current status of the Sub Agent.
/// - `start_time_unix_nano`: A `u64` representing the start time of the agent in nanoseconds since the Unix epoch.
/// - `status_time_unix_nano`: A `u64` representing the last status update time in nanoseconds since the Unix epoch.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub(super) struct HealthInfo {
    healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
    start_time_unix_nano: u64,
    status_time_unix_nano: u64,
}

impl SubAgentStatus {
    pub fn with_identity(agent_identity: AgentIdentity) -> Self {
        Self {
            agent_id: agent_identity.id,
            agent_type: agent_identity.agent_type_id,
            agent_start_time_unix_nano: 0,
            health_info: None,
            attributes: Default::default(),
        }
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        Self {
            agent_start_time_unix_nano: time_to_unix_timestamp(start_time),
            ..self
        }
    }

    // This struct only has context inside the Sub Agents struct, so it makes it easier to interact
    // if we make it mutable
    pub fn update_health(&mut self, health: HealthWithStartTime) {
        self.health_info = Some(HealthInfo {
            healthy: health.is_healthy(),
            last_error: health.last_error(),
            status: health.status().to_string(),
            start_time_unix_nano: time_to_unix_timestamp(health.start_time()),
            status_time_unix_nano: time_to_unix_timestamp(health.status_time()),
        });
    }
}

pub(super) type SubAgentsStatus = HashMap<AgentID, SubAgentStatus>;

/// Agent Control, Sub Agents and OpAMP status and health.
/// This information will be shown when the status endpoint is called.
///
/// Example: see [tests::test_status_serialization]
#[derive(Debug, Serialize, PartialEq, Default)]
pub(super) struct Status {
    pub(super) agent_control: AgentControlStatus,
    pub(super) fleet: OpAMPStatus,
    pub(super) agents: SubAgentsStatus,
}

impl Status {
    pub fn with_opamp(mut self, endpoint: Url) -> Self {
        self.fleet.enabled = true;
        self.fleet.endpoint = Some(endpoint);
        self
    }
}

fn time_to_unix_timestamp(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;

    use opamp_client::operation::settings::{AgentDescription, DescriptionValueType};
    use rstest::rstest;
    use serde_json::json;
    use url::Url;

    use crate::agent_control::agent_id::AgentID;

    use crate::agent_control::http_server::status::{
        AgentAttributes, AgentControlStatus, HealthInfo, OpAMPStatus, Status, SubAgentStatus,
        SubAgentsStatus, build_agent_attributes,
    };
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::opamp::{LastErrorCode, LastErrorMessage};

    impl Status {
        pub fn with_sub_agents(self, sub_agents: SubAgentsStatus) -> Self {
            Self {
                agents: sub_agents,
                ..self
            }
        }
    }

    impl AgentControlStatus {
        pub fn new_healthy(status: String) -> Self {
            AgentControlStatus {
                healthy: true,
                last_error: None,
                status,
                attributes: Default::default(),
            }
        }
        pub fn new_unhealthy(status: String, last_error: String) -> Self {
            AgentControlStatus {
                healthy: false,
                last_error: Some(last_error),
                status,
                attributes: Default::default(),
            }
        }
        pub fn with_attributes(mut self, attrs: HashMap<String, String>) -> Self {
            self.attributes = attrs;
            self
        }
    }

    impl SubAgentStatus {
        pub fn new(
            agent_id: AgentID,
            agent_type: AgentTypeID,
            agent_start_time_unix_nano: u64,
            health_info: HealthInfo,
        ) -> Self {
            SubAgentStatus {
                agent_id,
                agent_type,
                agent_start_time_unix_nano,
                health_info: Some(health_info),
                attributes: Default::default(),
            }
        }

        pub fn with_attributes(mut self, attrs: HashMap<String, String>) -> Self {
            self.attributes = attrs;
            self
        }
    }

    impl HealthInfo {
        pub fn new(
            status: String,
            healthy: bool,
            last_error: Option<String>,
            start_time_unix_nano: u64,
            status_time_unix_nano: u64,
        ) -> Self {
            HealthInfo {
                status,
                healthy,
                last_error,
                start_time_unix_nano,
                status_time_unix_nano,
            }
        }
    }

    impl OpAMPStatus {
        pub fn enabled_and_reachable(endpoint: Option<Url>) -> Self {
            OpAMPStatus {
                enabled: true,
                endpoint,
                reachable: true,
                error_code: None,
                error_message: None,
            }
        }
        pub fn enabled_and_unreachable(
            endpoint: Option<Url>,
            error_code: LastErrorCode,
            error_message: LastErrorMessage,
        ) -> Self {
            OpAMPStatus {
                enabled: true,
                endpoint,
                reachable: false,
                error_code: Some(error_code),
                error_message: Some(error_message),
            }
        }
    }

    #[test]
    fn test_status_serialization() {
        let status = Status {
            agent_control: AgentControlStatus {
                healthy: true,
                last_error: None,
                status: "".to_string(),
                attributes: Default::default(),
            },
            fleet: OpAMPStatus {
                enabled: true,
                endpoint: Some("https://opamp.server/v1/opamp".parse().unwrap()),
                reachable: true,
                error_code: None,
                error_message: None,
            },
            agents: SubAgentsStatus::from([
                (
                    AgentID::try_from("agent-id-1").unwrap(),
                    SubAgentStatus {
                        agent_id: AgentID::try_from("agent-id-1").unwrap(),
                        agent_type: AgentTypeID::try_from("ns/some.type:1.2.3").unwrap(),
                        agent_start_time_unix_nano: 0,
                        health_info: None,
                        attributes: Default::default(),
                    },
                ),
                (
                    AgentID::try_from("agent-id-2").unwrap(),
                    SubAgentStatus {
                        agent_id: AgentID::try_from("agent-id-2").unwrap(),
                        agent_type: AgentTypeID::try_from("ns/some.type:1.2.3").unwrap(),
                        agent_start_time_unix_nano: 0,
                        health_info: Some(HealthInfo {
                            healthy: true,
                            last_error: None,
                            status: "".to_string(),
                            start_time_unix_nano: 0,
                            status_time_unix_nano: 0,
                        }),
                        attributes: Default::default(),
                    },
                ),
                (
                    AgentID::try_from("agent-id-3").unwrap(),
                    SubAgentStatus {
                        agent_id: AgentID::try_from("agent-id-3").unwrap(),
                        agent_type: AgentTypeID::try_from("ns/some.type:1.2.3").unwrap(),
                        agent_start_time_unix_nano: 0,
                        health_info: Some(HealthInfo {
                            healthy: false,
                            last_error: Some("some error".to_string()),
                            status: "some error status".to_string(),
                            start_time_unix_nano: 0,
                            status_time_unix_nano: 0,
                        }),
                        attributes: Default::default(),
                    },
                ),
            ]),
        };
        let expected = serde_json::json!({
            "agent_control": {
                "healthy": true,
            },
            "fleet": {
                "enabled": true,
                "endpoint": "https://opamp.server/v1/opamp",
                "reachable": true,
            },
            "agents": {
                "agent-id-1": {
                    "agent_id": "agent-id-1",
                    "agent_type": "ns/some.type:1.2.3",
                    "agent_start_time_unix_nano": 0
                },
                "agent-id-2": {
                    "agent_id": "agent-id-2",
                    "agent_type": "ns/some.type:1.2.3",
                    "agent_start_time_unix_nano": 0,
                    "health_info": {
                        "healthy":true,
                        "start_time_unix_nano": 0,
                        "status_time_unix_nano": 0
                    },
                },
                "agent-id-3": {
                    "agent_id": "agent-id-3",
                    "agent_type": "ns/some.type:1.2.3",
                    "agent_start_time_unix_nano": 0,
                    "health_info": {
                        "healthy": false,
                        "status": "some error status",
                        "last_error": "some error",
                        "start_time_unix_nano": 0,
                        "status_time_unix_nano": 0
                    },
                },
            }
        });
        assert_eq!(serde_json::to_value(&status).unwrap(), expected);
    }

    #[test]
    fn test_attributes_omitted_when_empty() {
        let status = AgentControlStatus::default();
        let value = serde_json::to_value(&status).unwrap();
        assert!(!value.as_object().unwrap().contains_key("attributes"));
    }

    #[test]
    fn test_attributes_serialized_when_populated() {
        let mut attributes = AgentAttributes::new();
        attributes.insert("version".to_string(), "1.2.3".to_string());
        attributes.insert(
            "instance_uid".to_string(),
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
        );
        let status = AgentControlStatus {
            attributes,
            ..Default::default()
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(
            value["attributes"],
            json!({
                "version": "1.2.3",
                "instance_uid": "550e8400-e29b-41d4-a716-446655440000"
            })
        );
    }

    #[rstest]
    #[case::empty(HashMap::new(), HashMap::new(), vec![])]
    #[case::non_identifying_only(
        HashMap::new(),
        HashMap::from([("k".to_string(), DescriptionValueType::String("v".to_string()))]),
        vec![("non-identifying/k", "v")]
    )]
    #[case::identifying_only(
        HashMap::from([("k".to_string(), DescriptionValueType::String("v".to_string()))]),
        HashMap::new(),
        vec![("identifying/k", "v")]
    )]
    #[case::merged_no_overlap(
        HashMap::from([("id".to_string(), DescriptionValueType::String("id_val".to_string()))]),
        HashMap::from([("non_id".to_string(), DescriptionValueType::String("non_id_val".to_string()))]),
        vec![("identifying/id", "id_val"), ("non-identifying/non_id", "non_id_val")]
    )]
    #[case::both_present_when_key_in_both(
        HashMap::from([("shared".to_string(), DescriptionValueType::String("id_value".to_string()))]),
        HashMap::from([("shared".to_string(), DescriptionValueType::String("non_id_value".to_string()))]),
        vec![("identifying/shared", "id_value"), ("non-identifying/shared", "non_id_value")]
    )]
    fn test_agent_description(
        #[case] identifying_attributes: HashMap<String, DescriptionValueType>,
        #[case] non_identifying_attributes: HashMap<String, DescriptionValueType>,
        #[case] expected: Vec<(&str, &str)>,
    ) {
        let agent_description = AgentDescription {
            identifying_attributes,
            non_identifying_attributes,
        };
        let result = build_agent_attributes(agent_description);
        assert_eq!(result.len(), expected.len());
        for (key, val) in expected {
            assert_eq!(result.get(key).map(String::as_str), Some(val));
        }
    }

    #[rstest]
    #[case::string(DescriptionValueType::String("hello".to_string()), "hello")]
    #[case::int(DescriptionValueType::Int(42), "42")]
    #[case::bool(DescriptionValueType::Bool(true), "true")]
    #[case::float(DescriptionValueType::Float(4.13), "4.13")]
    #[case::bytes(DescriptionValueType::Bytes(vec![0xde, 0xad, 0xbe, 0xef]), "deadbeef")]
    fn test_agent_description_encode_types(
        #[case] value: DescriptionValueType,
        #[case] expected: &str,
    ) {
        let agent_description = AgentDescription {
            identifying_attributes: HashMap::from([("k".to_string(), value)]),
            non_identifying_attributes: Default::default(),
        };

        assert_eq!(
            build_agent_attributes(agent_description)
                .get("identifying/k")
                .map(String::as_str),
            Some(expected)
        );
    }
}
