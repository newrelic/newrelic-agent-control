/// Namespace defines the supported namespace names for variables definition.
pub enum Namespace {
    Variable,
    SubAgent,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    const VARIABLE: &'static str = "var";
    const SUB_AGENT: &'static str = "sub";

    pub fn namespaced_name(&self, name: &str) -> String {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
        };
        format!("{}{ns}:{name}", Self::PREFIX)
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
    }
}
