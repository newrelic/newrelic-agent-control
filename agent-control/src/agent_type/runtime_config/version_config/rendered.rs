use regex::Regex;

pub(crate) use crate::agent_type::runtime_config::on_host::executable::rendered::Args;

#[derive(Debug, Clone)]
pub struct OnHostVersionConfig {
    /// Path to the binary from which we want to check the version.
    pub path: String,
    // Command arguments.
    pub args: Args,
    /// The regex expression to get the version from the command output.
    ///
    /// If not provided, the entire output will be used.
    pub(crate) regex: Option<Regex>,
}

impl PartialEq for OnHostVersionConfig {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.args == other.args
            && self.regex.as_ref().map(|r| r.as_str()) == other.regex.as_ref().map(|r| r.as_str())
    }
}
