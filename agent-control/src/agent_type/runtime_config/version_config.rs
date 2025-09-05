use serde::Deserialize;

use crate::agent_type::{
    definition::Variables,
    error::AgentTypeError,
    runtime_config::{onhost::Args, templateable_value::TemplateableValue},
    templates::Templateable,
};

/// Represents the configuration for version checks.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct OnHostVersionConfig {
    /// Path to the binary from which we want to check the version.
    pub path: String,

    // Command arguments.
    pub args: TemplateableValue<Args>,

    /// The regex expression to get the version from the command output.
    ///
    /// If not provided, the entire output will be used.
    pub(crate) regex: Option<String>,
}

impl Templateable for OnHostVersionConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            regex: self.regex,
        })
    }
}
