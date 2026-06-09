use crate::agent_type::definition::{AgentTypeDefinition, parse_agent_type_definition};
use crate::environment::Environment;

// Include generated code
include!(concat!(
    env!("OUT_DIR"), // set by Cargo
    "/",
    env!("GENERATED_REGISTRY_FILE"), // Set in the agent-control build script
));

/// Iterates the agent-type definitions embedded into the binary at compilation time that match the
/// given [Environment].
///
/// The definitions come from the yaml files embedded by the agent-control build script. They are
/// expected to be valid, hence this function panics if any of them cannot be deserialized.
/// Definitions whose environment does not match `env` are skipped, so the running binary only sees
/// the agent types it can actually use.
pub(super) fn embedded_definitions(env: Environment) -> impl Iterator<Item = AgentTypeDefinition> {
    AGENT_TYPE_REGISTRY_FILES
        .iter()
        .map(|file_content_ref| {
            // Definitions in files are expected to be valid and protocol-compatible.
            parse_agent_type_definition(file_content_ref)
                .expect("Invalid or incompatible embedded agent type")
        })
        .filter(move |def| def.metadata.environment == env)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-environment counts of embedded agent type definitions. Updating these requires
    /// adding/removing files in `agent-type-registry/`.
    const KUBERNETES_AGENT_TYPE_AMOUNT: usize = 15;
    const LINUX_AGENT_TYPE_AMOUNT: usize = 4;
    const WINDOWS_AGENT_TYPE_AMOUNT: usize = 2;

    #[test]
    fn embedded_definitions_count_per_environment() {
        // Parsing every embedded yaml panics on any invalid one, and the per-environment counts
        // flag in CI if any agent type has been added or removed for a given environment.
        assert_eq!(
            embedded_definitions(Environment::K8s).count(),
            KUBERNETES_AGENT_TYPE_AMOUNT,
            "Expected amount of kubernetes agent types to be unchanged"
        );
        assert_eq!(
            embedded_definitions(Environment::Linux).count(),
            LINUX_AGENT_TYPE_AMOUNT,
            "Expected amount of linux agent types to be unchanged"
        );
        assert_eq!(
            embedded_definitions(Environment::Windows).count(),
            WINDOWS_AGENT_TYPE_AMOUNT,
            "Expected amount of windows agent types to be unchanged"
        );
    }
}
