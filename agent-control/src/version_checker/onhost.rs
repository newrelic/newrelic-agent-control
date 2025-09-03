use std::process::Command;

use crate::agent_control::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
use crate::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use regex::Regex;

pub struct OnHostAgentVersionChecker {
    pub(crate) command: String,
    pub(crate) regex: Option<String>,
}

impl VersionChecker for OnHostAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        let command_data = self.command.split_ascii_whitespace().collect::<Vec<&str>>();
        let program = command_data[0];
        let args = command_data.get(1..).unwrap_or(&[]);

        let output = Command::new(program).args(args).output().map_err(|e| {
            VersionCheckError::Generic(format!("error executing version command: {e}"))
        })?;
        let output = String::from_utf8_lossy(&output.stdout);

        let version = if let Some(pattern) = &self.regex {
            let regex = Regex::new(pattern)
                .map_err(|e| VersionCheckError::Generic(format!("error compiling regex: {e}")))?;

            let version_match = regex.find(&output).ok_or(VersionCheckError::Generic(
                "error checking agent version: version not found".to_string(),
            ))?;
            version_match.as_str().to_string()
        } else {
            output.to_string()
        };

        Ok(AgentVersion {
            version: version.as_str().to_string(),
            opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;

    #[rstest]
    #[case::command_and_regex("echo \"Some data 1.0.0 Some more data\"", Some(r"\d+\.\d+\.\d+"))]
    #[case::command_and_regex("echo -n 1.0.0", None)]
    fn test_check_agent_version(#[case] command: &str, #[case] regex: Option<&str>) {
        let agent_version = OnHostAgentVersionChecker {
            command: command.to_string(),
            regex: regex.map(|r| r.to_string()),
        }
        .check_agent_version()
        .unwrap();

        assert_eq!(agent_version.version.as_str(), "1.0.0");
        assert_eq!(
            agent_version.opamp_field.as_str(),
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
        );
    }
}
