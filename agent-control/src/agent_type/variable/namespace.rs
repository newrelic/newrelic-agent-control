use std::{fmt::Display, str::FromStr};

/// Holds the variable name prefixed with the namespace.
/// Example: "nr-env:MY_ENV_VAR" for the environment variable "MY_ENV_VAR".
pub type NamespacedVariableName = String;

/// Namespace defines the supported namespace names for variables definition.
pub enum Namespace {
    Variable,
    SubAgent,
    AgentControl,

    // Below variables are "runtime" variables.
    // When we receive a config, the config could have new environment variables, for example.
    // These kind of variables must be loaded every time the subagent is started.
    EnvironmentVariable,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    /// Encapsulates the variables defined in the agent-type
    const VARIABLE: &'static str = "var";
    /// Encapsulates attributes related to the sub-agent
    const SUB_AGENT: &'static str = "sub";
    /// Encapsulates attributes related to the agent-control
    const AC: &'static str = "ac";

    /// Encapsulates the environment variables that are available to the sub-agent
    const ENVIRONMENT_VARIABLE: &'static str = "env";

    pub const PREFIX_NS_SEPARATOR: &'static str = ":";

    pub fn namespaced_name(&self, name: &str) -> NamespacedVariableName {
        format!("{}{}{}", self, Self::PREFIX_NS_SEPARATOR, name)
    }

    pub fn is_runtime_variable(s: &str) -> bool {
        let prefix = s.split(Self::PREFIX_NS_SEPARATOR).next();
        let Some(namespace) = prefix.map(Namespace::from_str).map(Result::ok).flatten() else {
            return false;
        };

        match namespace {
            Namespace::EnvironmentVariable => true,
            _ => false,
        }
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
            Self::AgentControl => Self::AC,
            Self::EnvironmentVariable => Self::ENVIRONMENT_VARIABLE,
        };
        write!(f, "{}{ns}", Self::PREFIX)
    }
}

impl FromStr for Namespace {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(Self::PREFIX) {
            return Err(anyhow::anyhow!(
                "Namespace must start with '{}'",
                Self::PREFIX
            ));
        }

        match &s[Self::PREFIX.len()..] {
            Self::VARIABLE => Ok(Self::Variable),
            Self::SUB_AGENT => Ok(Self::SubAgent),
            Self::AC => Ok(Self::AgentControl),
            Self::ENVIRONMENT_VARIABLE => Ok(Self::EnvironmentVariable),
            _ => Err(anyhow::anyhow!("Unknown namespace: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_namespaced_name() {
        assert_eq!(
            "nr-var:test".to_string(),
            Namespace::Variable.namespaced_name("test")
        );
        assert_eq!(
            "nr-sub:test".to_string(),
            Namespace::SubAgent.namespaced_name("test")
        );
        assert_eq!(
            "nr-env:test".to_string(),
            Namespace::EnvironmentVariable.namespaced_name("test")
        );
        assert_eq!(
            "nr-ac:test".to_string(),
            Namespace::AgentControl.namespaced_name("test")
        );
    }
}
