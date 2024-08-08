/// Holds the variable name prefixed with the namespace.
/// Example: "nr-env:MY_ENV_VAR" for the environment variable "MY_ENV_VAR".
pub type NamespacedVariableName = String;

/// Namespace defines the supported namespace names for variables definition.
pub enum Namespace {
    Variable,
    SubAgent,
    EnvironmentVariable,
    SuperAgent,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    /// Encapsulates the variables defined in the agent-type
    const VARIABLE: &'static str = "var";
    /// Encapsulates the environment variables that are available to the sub-agent
    const ENVIRONMENT_VARIABLE: &'static str = "env";
    /// Encapsulates attributes related to the sub-agent
    const SUB_AGENT: &'static str = "sub";
    /// Encapsulates attributes related to the super-agent
    const SA: &'static str = "sa";

    pub fn namespaced_name(&self, name: &str) -> NamespacedVariableName {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::EnvironmentVariable => Self::ENVIRONMENT_VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
            Self::SuperAgent => Self::SA,
        };
        format!("{}{}:{}", Self::PREFIX, ns, name)
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
            "nr-sa:test".to_string(),
            Namespace::SuperAgent.namespaced_name("test")
        );
    }
}
