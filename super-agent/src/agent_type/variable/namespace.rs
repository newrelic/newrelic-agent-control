/// Namespace defines the supported namespace names for variables definition.
pub enum Namespace {
    Variable,
    SubAgent,
    EnvironmentVariable,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    const VARIABLE: &'static str = "var";
    const ENVIRONMENT_VARIABLE: &'static str = "env";
    const SUB_AGENT: &'static str = "sub";

    pub fn namespaced_name(&self, name: &str) -> String {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::EnvironmentVariable => Self::ENVIRONMENT_VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
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
    }
}
