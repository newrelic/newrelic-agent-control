use crate::agent_type::definition::AgentTypeDefinition;

// Include generated code
include!(concat!(
    env!("OUT_DIR"), // set by Cargo
    "/",
    env!("GENERATED_REGISTRY_FILE"), // Set in the agent-control build script
));

/// Iterates the agent-type definitions embedded into the binary at compilation time.
///
/// The definitions come from the yaml files embedded by the agent-control build script. They are
/// expected to be valid, hence this function panics if any of them cannot be deserialized.
pub(super) fn embedded_definitions() -> impl Iterator<Item = AgentTypeDefinition> {
    AGENT_TYPE_REGISTRY_FILES.iter().map(|file_content_ref| {
        // Definitions in files are expected to be valid
        serde_saphyr::from_reader::<_, AgentTypeDefinition>(file_content_ref.to_owned())
            .expect("Invalid yaml in default agent types")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT_TYPE_AMOUNT: usize = 14;

    #[test]
    fn embedded_definitions_parse_one_per_file() {
        // Calling the loader parses every embedded yaml (it panics on any invalid one) and must
        // yield exactly one definition per embedded file.
        let definitions = embedded_definitions().collect::<Vec<_>>();
        // This is intended to flag in CI if any agent type has been added or removed.
        // Changes in code that modify the amount of agent types would need to modify this test.
        assert_eq!(
            definitions.len(),
            AGENT_TYPE_AMOUNT,
            "Expected amount of agent types to be unchanged"
        );
    }
}
