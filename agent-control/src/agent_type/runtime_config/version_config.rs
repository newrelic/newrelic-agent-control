use serde::Deserialize;

use crate::agent_type::{
    definition::Variables, error::AgentTypeError, templates::Templateable,
    version_config::VersionCheckerInterval,
};

/// Represents the configuration for version checks.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct OnHostVersionConfig {
    /// The command to check the version.
    #[serde(default)]
    pub(crate) command: String,

    /// The regex expression to get the version from the command output.
    ///
    /// If not provided, the entire output will be used.
    #[serde(default)]
    pub(crate) regex: Option<String>,

    /// The duration to wait between version checks.
    #[serde(default)]
    pub(crate) interval: VersionCheckerInterval,
}

impl Templateable for OnHostVersionConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            command: self.command.template_with(variables)?,
            regex: self.regex,
            interval: self.interval,
        })
    }
}
