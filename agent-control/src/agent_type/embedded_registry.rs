use std::{collections::HashMap, path::PathBuf};

use std::fs;
use tracing::{debug, error, info};

use super::{
    agent_type_registry::{AgentRegistry, AgentRepositoryError},
    definition::AgentTypeDefinition,
};

// Include generated code
include!(concat!(
    env!("OUT_DIR"), // set by Cargo
    "/",
    env!("GENERATED_REGISTRY_FILE"), // Set in the agent-control build script
));

/// Defines an [AgentRegistry] by keeping AgentTypeDefinitions in memory.
///
/// Its default implementation, loads the AgentTypeDefinitions from yaml files which are embedded into the binary
/// at compilation time. Check out the agent-control build script for details.
#[derive(Debug)]
pub struct EmbeddedRegistry(HashMap<String, AgentTypeDefinition>);

impl Default for EmbeddedRegistry {
    fn default() -> Self {
        Self::try_new(Self::definitions()).expect("Conflicting agent type definitions")
    }
}

impl EmbeddedRegistry {
    pub fn new(dynamic_agent_type_path: PathBuf) -> Self {
        let dynamic_agent_type = Self::dynamic_agent_type(dynamic_agent_type_path);
        let definitions = Self::definitions().chain(dynamic_agent_type);
        Self::try_new(definitions).expect("Conflicting agent type definitions")
    }
}

impl AgentRegistry for EmbeddedRegistry {
    fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError> {
        self.0
            .get(name)
            .cloned()
            .ok_or(AgentRepositoryError::NotFound)
    }
}

impl EmbeddedRegistry {
    fn try_new<T: IntoIterator<Item = AgentTypeDefinition>>(
        definitions_iter: T,
    ) -> Result<Self, AgentRepositoryError> {
        let mut registry = Self(HashMap::new());
        definitions_iter
            .into_iter()
            .try_for_each(|definition| registry.insert(definition))?;
        Ok(registry)
    }

    fn insert(&mut self, definition: AgentTypeDefinition) -> Result<(), AgentRepositoryError> {
        let metadata = definition.metadata.to_string();
        if self.0.contains_key(&metadata) {
            return Err(AgentRepositoryError::AlreadyExists(metadata));
        }
        self.0.insert(metadata, definition);
        Ok(())
    }

    /// Iters the embedded agent-type definitions.
    fn definitions() -> impl Iterator<Item = AgentTypeDefinition> {
        AGENT_TYPE_REGISTRY_FILES.iter().map(|file_content_ref| {
            // Definitions in files are expected to be valid
            serde_yaml::from_reader::<_, AgentTypeDefinition>(file_content_ref.to_owned())
                .expect("Invalid yaml in default agent types")
        })
    }

    /// Read and return the dynamic agent type, if there is an error reading or deserializing it, logs the error and
    /// returns None.
    fn dynamic_agent_type(path: PathBuf) -> Option<AgentTypeDefinition> {
        let p = path.to_string_lossy().to_string();
        fs::read(path)
            .inspect_err(|e| {
                debug!(error = %e, "Dynamic agent type: Failed reading file");
            })
            .ok()
            .and_then(|content| {
                info!("Loading agentType : {:?}", p);
                serde_yaml::from_slice::<AgentTypeDefinition>(content.as_slice())
                    .inspect_err(|e| {
                        error!(error = %e, "Dynamic agent type: Could not parse agent type");
                    })
                    .ok()
            })
    }
}

#[cfg(test)]
pub mod tests {
    use assert_matches::assert_matches;
    use semver::Version;

    use crate::agent_type::agent_metadata::AgentMetadata;

    use super::*;

    const AGENT_TYPE_AMOUNT: usize = 11;

    #[test]
    fn check_agent_type_amount_is_unchanged() {
        // This is intended to flag in CI if any agent type has been added or removed.
        // Changes in code that modify the amount of agent types would need to modify this test.
        assert_eq!(
            AGENT_TYPE_REGISTRY_FILES.len(),
            AGENT_TYPE_AMOUNT,
            "Expected amount of agent types to be unchanged"
        );
    }

    #[test]
    fn test_default_embedded_registry() {
        let registry = EmbeddedRegistry::default(); // Any invalid Agent Type definition would panic

        assert_eq!(
            AGENT_TYPE_REGISTRY_FILES.len(),
            registry.0.len(),
            "expected one AgentTypeDefinition for each file"
        );

        // The expected key for each definition should be the metadata string
        for (key, definition) in registry.0.iter() {
            assert_eq!(key.to_string(), definition.metadata.to_string())
        }

        let registry_nonexistent_dynamic =
            EmbeddedRegistry::new(PathBuf::from("/nonexistent/path"));
        assert_eq!(
            registry.0, registry_nonexistent_dynamic.0,
            "Registry with nonexistent dynamic should match default"
        )
    }

    #[test]
    fn test_get() {
        let definitions = vec![
            AgentTypeDefinition::empty_with_metadata(AgentMetadata {
                name: "agent-1".into(),
                version: Version::parse("0.0.0").unwrap(),
                namespace: "ns".into(),
            }),
            AgentTypeDefinition::empty_with_metadata(AgentMetadata {
                name: "agent-2".into(),
                version: Version::parse("0.0.0").unwrap(),
                namespace: "ns".into(),
            }),
        ];

        let registry = EmbeddedRegistry::try_new(definitions.clone()).unwrap();

        let agent_1 = registry.get("ns/agent-1:0.0.0").unwrap();
        assert_eq!(definitions[0], agent_1);
        let agent_2 = registry.get("ns/agent-2:0.0.0").unwrap();
        assert_eq!(definitions[1], agent_2);

        let err = registry.get("not-existent").unwrap_err();
        assert_matches!(err, AgentRepositoryError::NotFound);
    }

    #[test]
    fn test_insert_duplicate() {
        let mut registry = EmbeddedRegistry::default();

        let definition = AgentTypeDefinition::empty_with_metadata(AgentMetadata {
            name: "agent".into(),
            version: Version::parse("0.0.0").unwrap(),
            namespace: "ns".into(),
        });
        let duplicate = definition.clone();

        assert!(registry.insert(definition).is_ok());

        let err = registry.insert(duplicate).unwrap_err();
        assert_matches!(err, AgentRepositoryError::AlreadyExists(name) => {
            assert_eq!("ns/agent:0.0.0", name);
        })
    }
}
