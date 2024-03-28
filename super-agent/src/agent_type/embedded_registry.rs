use std::collections::HashMap;

use rust_embed::RustEmbed;

use super::{
    agent_type_registry::{AgentRegistry, AgentRepositoryError},
    definition::AgentTypeDefinition,
};

#[derive(RustEmbed)]
#[folder = "agent-type-registry/"]
#[include = "*.yaml"]
struct RegistryEmbeddedFiles;

/// Defines an [AgentRegistry] by keeping AgentTypeDefinitions in memory.
/// Its default implementation, loads the AgentTypeDefinitions from yaml files which are embedded into the binary
/// at compilation time. Check out [RustEmbed] for details.
#[derive(Debug)]
pub struct EmbeddedRegistry(HashMap<String, AgentTypeDefinition>);

impl Default for EmbeddedRegistry {
    fn default() -> Self {
        RegistryEmbeddedFiles::iter()
            .map(|file_path| {
                // Listed files always exist
                let file = RegistryEmbeddedFiles::get(&file_path).unwrap();
                // Definitions in files are expected to be valid
                serde_yaml::from_reader::<_, AgentTypeDefinition>(file.data.as_ref()).unwrap()
            })
            .collect()
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

impl FromIterator<AgentTypeDefinition> for EmbeddedRegistry {
    fn from_iter<T: IntoIterator<Item = AgentTypeDefinition>>(iter: T) -> Self {
        Self(
            iter.into_iter()
                .map(|definition| (definition.metadata.to_string(), definition))
                .collect(),
        )
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
            RegistryEmbeddedFiles::iter().count(),
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

        let registry: EmbeddedRegistry = definitions.clone().into_iter().collect();

        let agent_1 = registry.get("ns/agent-1:0.0.0").unwrap();
        assert_eq!(definitions[0], agent_1);
        let agent_2 = registry.get("ns/agent-2:0.0.0").unwrap();
        assert_eq!(definitions[1], agent_2);

        let err = registry.get("not-existent").unwrap_err();
        assert_matches!(err, AgentRepositoryError::NotFound);
    }
}
