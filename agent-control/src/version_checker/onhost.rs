use crate::agent_control::defaults::{
    AGENT_TYPE_NAME_EBPF, AGENT_TYPE_NAME_INFRA_AGENT, AGENT_TYPE_NAME_NRDOT,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use tracing::error;

pub struct OnHostAgentVersionChecker {
    agent_version: AgentVersion,
}

impl OnHostAgentVersionChecker {
    pub fn checked_new(agent_type_id: AgentTypeID) -> Option<Self> {
        match retrieve_version(&agent_type_id) {
            Ok(agent_version) => Some(Self { agent_version }),
            Err(e) => {
                error!("error checking agent version: {}", e);
                None
            }
        }
    }
}

impl VersionChecker for OnHostAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        Ok(self.agent_version.clone())
    }
}

fn retrieve_version(agent_type_id: &AgentTypeID) -> Result<AgentVersion, VersionCheckError> {
    let name = agent_type_id.name();
    let version = agent_type_id.version();

    match name {
        AGENT_TYPE_NAME_INFRA_AGENT | AGENT_TYPE_NAME_NRDOT | AGENT_TYPE_NAME_EBPF => {
            Ok(AgentVersion {
                version: format!("{}.{}.{}", version.major, version.minor, version.patch),
                opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
            })
        }
        _ => Err(VersionCheckError::Generic(format!(
            "no match found for agent type: {agent_type_id}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use crate::sub_agent::identity::AgentIdentity;

    use super::*;

    use assert_matches::assert_matches;

    #[test]
    fn test_agent_version_checker_build() {
        struct TestCase {
            name: &'static str,
            agent_type_id: AgentTypeID,
            check: fn(&'static str, Option<OnHostAgentVersionChecker>),
        }

        impl TestCase {
            fn run(self) {
                let result = OnHostAgentVersionChecker::checked_new(self.agent_type_id);
                let check = self.check;
                check(self.name, result);
            }
        }

        let test_cases = [
            TestCase {
                name: "Version cannot be computed for the superAgent",
                agent_type_id: AgentIdentity::new_agent_control_identity().agent_type_id,
                check: |name, result| {
                    assert!(result.is_none(), "{name}",);
                },
            },
            TestCase {
                name: "infrastructure agent version is computed correctly ",
                agent_type_id: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0")
                    .unwrap(),
                check: |name, result| {
                    let r = result.unwrap();
                    assert_matches!(
                        r.check_agent_version().unwrap().version.as_str(),
                        "0.1.0",
                        "{name}",
                    );
                    assert_matches!(
                        r.check_agent_version().unwrap().opamp_field.as_str(),
                        OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
                        "{name}",
                    );
                },
            },
        ];
        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
