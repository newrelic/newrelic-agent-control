use regex::Regex;
use serde::{Deserialize, Deserializer};

use crate::agent_type::{
    definition::Variables,
    error::AgentTypeError,
    runtime_config::{on_host::executable::Args, templateable_value::TemplateableValue},
    templates::Templateable,
};

pub mod rendered;

/// Represents the configuration for version checks.
#[derive(Debug, Clone)]
pub struct OnHostVersionConfig {
    /// Path to the binary from which we want to check the version.
    pub path: TemplateableValue<String>,

    // Command arguments.
    pub args: TemplateableValue<Args>,

    /// The regex expression to get the version from the command output.
    ///
    /// If not provided, the entire output will be used.
    pub(crate) regex: Option<Regex>,
}

impl<'de> Deserialize<'de> for OnHostVersionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // intermediate serialization type to validate `default` and `required` fields
        #[derive(Debug, Deserialize)]
        pub struct IntermediateOnHostVersionConfig {
            path: TemplateableValue<String>,
            args: TemplateableValue<Args>,
            regex: Option<String>,
        }

        let intermediate_spec = IntermediateOnHostVersionConfig::deserialize(deserializer)?;

        let regex = intermediate_spec
            .regex
            .as_ref()
            .map(|r| {
                Regex::new(r)
                    .map_err(|e| serde::de::Error::custom(format!("error compiling regex: {e}")))
            })
            .transpose()?;

        Ok(OnHostVersionConfig {
            path: intermediate_spec.path,
            args: intermediate_spec.args,
            regex,
        })
    }
}

impl PartialEq for OnHostVersionConfig {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.args == other.args
            && self.regex.as_ref().map(|r| r.as_str()) == other.regex.as_ref().map(|r| r.as_str())
    }
}

impl Templateable for OnHostVersionConfig {
    type Output = rendered::OnHostVersionConfig;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            regex: self.regex,
        })
    }
}
