use std::collections::HashMap;

use crate::super_agent::defaults::DYNAMIC_AGENT_TYPE;
use std::fs;
use tracing::debug;

use super::{
    agent_type_registry::{AgentRegistry, AgentRepositoryError},
    definition::AgentTypeDefinition,
};

// Include generated code
include!(concat!(
    env!("OUT_DIR"), // set by Cargo
    "/",
    env!("GENERATED_REGISTRY_FILE"), // Set in the super-agent build script
));

/// Defines an [AgentRegistry] by keeping AgentTypeDefinitions in memory.
/// Its default implementation, loads the AgentTypeDefinitions from yaml files which are embedded into the binary
/// at compilation time. Check out the super-agent build script for details.
#[derive(Debug)]
pub struct EmbeddedRegistry(HashMap<String, AgentTypeDefinition>);

impl Default for EmbeddedRegistry {
    fn default() -> Self {
        let definitions = AGENT_TYPE_REGISTRY_FILES.iter().map(|file_content_ref| {
            // Definitions in files are expected to be valid
            serde_yaml::from_reader::<_, AgentTypeDefinition>(file_content_ref.to_owned())
                .expect("Invalid yaml in default agent types")
        });

        // Read the dynamic agent type and merge with the static ones.
        // Log failure but not fail the whole registry creation
        let dynamic_agent_type = fs::read(DYNAMIC_AGENT_TYPE)
            .inspect_err(|e| debug!("Failed to load dynamic agent type: {}", e))
            .ok()
            .and_then(|content| {
                serde_yaml::from_slice::<AgentTypeDefinition>(content.as_slice())
                    .inspect_err(|e| debug!("Failed to parse dynamic agent type: {}", e))
                    .ok()
            });

        let definitions = definitions.chain(dynamic_agent_type);

        Self::try_new(definitions).expect("Conflicting agent type definitions")
    }
}

impl AgentRegistry for EmbeddedRegistry {
    fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| AgentRepositoryError::NotFound)
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
}

#[cfg(test)]
pub mod tests {
    use assert_matches::assert_matches;

    use crate::agent_type::{
        agent_metadata::AgentMetadata,
        definition::{AgentTypeVariables, VariableTree},
        runtime_config::{Deployment, Runtime},
    };

    use super::*;

    impl AgentTypeDefinition {
        /// This helper returns an [AgentTypeDefinition] including only the provided metadata
        pub fn empty_with_metadata(metadata: AgentMetadata) -> Self {
            Self {
                metadata,
                variables: AgentTypeVariables {
                    common: VariableTree::default(),
                    k8s: VariableTree::default(),
                    on_host: VariableTree::default(),
                },
                runtime_config: Runtime {
                    deployment: Deployment {
                        on_host: None,
                        k8s: None,
                    },
                },
            }
        }
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
    }

    #[test]
    fn test_get() {
        let definitions = vec![
            AgentTypeDefinition::empty_with_metadata(AgentMetadata {
                name: "agent-1".into(),
                version: "0.0.0".into(),
                namespace: "ns".into(),
            }),
            AgentTypeDefinition::empty_with_metadata(AgentMetadata {
                name: "agent-2".into(),
                version: "0.0.0".into(),
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
            version: "0.0.0".into(),
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
