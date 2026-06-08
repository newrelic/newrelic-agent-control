use super::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::definition::AgentTypeDefinition;
use crate::environment::Environment;
use std::{collections::HashMap, fs, path::PathBuf};
use tracing::{debug, error};

// Include generated code
include!(concat!(
    env!("OUT_DIR"), // set by Cargo
    "/",
    env!("GENERATED_REGISTRY_FILE"), // Set in the agent-control build script
));

/// Defines an [AgentTypeRegistry] by keeping AgentTypeDefinitions in memory.
///
/// The embedded YAMLs cover several environments (linux, windows, kubernetes); definitions
/// not matching the requested [Environment] are skipped at construction time. This way the
/// registry only contains entries the running binary can actually use.
#[derive(Debug)]
pub struct EmbeddedRegistry(HashMap<AgentTypeID, AgentTypeDefinition>);

impl AgentTypeRegistry for EmbeddedRegistry {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        self.0
            .get(agent_type_id)
            .cloned()
            .ok_or_else(|| AgentTypeRegistryError::NotFound(agent_type_id.to_string()))
    }
}

impl EmbeddedRegistry {
    pub fn new(env: Environment, dynamic_agent_type_path: PathBuf) -> Self {
        // Loading the static agentTypes
        let mut registry = Self::try_new(Self::load_definitions_for(env))
            .expect("Conflicting agent type definitions");

        // Loading, if any, the dynamic agent types from the directory.
        // Since they are dynamic, they are taking the precedence over the static ones.
        Self::dynamic_agent_type(dynamic_agent_type_path)
            .into_iter()
            .filter(|agent_type| agent_type.metadata.environment == env)
            .for_each(|agent_type| {
                let id = agent_type.agent_type_id().clone();
                debug!("Storing dynamic agent type: {}", id);
                registry.0.insert(id, agent_type.clone());
            });
        registry
    }

    fn try_new<T: IntoIterator<Item = AgentTypeDefinition>>(
        definitions_iter: T,
    ) -> Result<Self, AgentTypeRegistryError> {
        let mut registry = Self(HashMap::new());
        definitions_iter
            .into_iter()
            .try_for_each(|definition| registry.insert(definition))?;
        Ok(registry)
    }

    fn insert(&mut self, definition: AgentTypeDefinition) -> Result<(), AgentTypeRegistryError> {
        let id = definition.agent_type_id().clone();
        if self.0.contains_key(&id) {
            return Err(AgentTypeRegistryError::AlreadyExists(id.to_string()));
        }
        self.0.insert(id, definition);
        Ok(())
    }

    /// Loads the embedded agent-type definitions matching the given [Environment].
    fn load_definitions_for(env: Environment) -> impl Iterator<Item = AgentTypeDefinition> {
        AGENT_TYPE_REGISTRY_FILES
            .iter()
            .map(|file_content_ref| {
                serde_saphyr::from_reader::<_, AgentTypeDefinition>(file_content_ref.to_owned())
                    .expect("Invalid yaml in default agent types")
            })
            .filter(move |def| def.metadata.environment == env)
    }

    /// Read and return the dynamic agent types, if there is an error reading or deserializing it, logs the error.
    fn dynamic_agent_type(path: PathBuf) -> Vec<AgentTypeDefinition> {
        let Ok(dir_entries) = fs::read_dir(path.clone()).inspect_err(
            |err| debug!(error = %err, "Failed reading Dynamic agent types directory {path:?}"),
        ) else {
            return vec![];
        };

        let mut entries: Vec<_> = dir_entries.flatten().collect();
        // The order of entries returned by the `dir_entries` iterator is platform and filesystem
        // dependent. To ensure a consistent order of processing, we sort the entries by their path.
        // This is important because the current implementation uses a HashMap, and inserting
        // already existing keys will overwrite the former values.
        entries.sort_by_key(|a| a.path());

        entries.into_iter()
            .flat_map(|entry| {
                let file = entry.path();
                fs::read(file.clone())
                    .inspect_err(|e| debug!(error = %e, "Skipping file: {file:?}"))
                    .ok()
                    .and_then(|content| {
                        debug!("Loading Dynamic Agent Type: {file:?}");
                        serde_saphyr::from_slice::<AgentTypeDefinition>(content.as_slice())
                            .inspect_err(|e| error!(error = %e, "Could not parse Dynamic Agent Type: {file:?}"))
                            .ok()
                    })
            })
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use assert_matches::assert_matches;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    impl EmbeddedRegistry {
        pub fn iter_definitions(&self) -> impl Iterator<Item = &AgentTypeDefinition> {
            self.0.values()
        }
    }

    impl From<AgentTypeDefinition> for EmbeddedRegistry {
        fn from(value: AgentTypeDefinition) -> Self {
            let mut registry = Self(HashMap::new());
            registry.insert(value).unwrap();
            registry
        }
    }

    /// Per-environment counts of embedded agent type definitions. Updating these requires
    /// adding/removing files in `agent-type-registry/`.
    const KUBERNETES_AGENT_TYPE_AMOUNT: usize = 15;
    const LINUX_AGENT_TYPE_AMOUNT: usize = 4;
    const WINDOWS_AGENT_TYPE_AMOUNT: usize = 2;

    #[test]
    fn check_agent_type_amount_is_unchanged() {
        // Flags in CI if any agent type has been added or removed for any environment.
        let kubernetes = EmbeddedRegistry::new(Environment::K8s, PathBuf::from("/nonexistent"));
        let linux = EmbeddedRegistry::new(Environment::Linux, PathBuf::from("/nonexistent"));
        let windows = EmbeddedRegistry::new(Environment::Windows, PathBuf::from("/nonexistent"));

        assert_eq!(KUBERNETES_AGENT_TYPE_AMOUNT, kubernetes.0.len());
        assert_eq!(LINUX_AGENT_TYPE_AMOUNT, linux.0.len());
        assert_eq!(WINDOWS_AGENT_TYPE_AMOUNT, windows.0.len());
    }

    #[test]
    fn test_embedded_registry_keys_match_metadata() {
        for env in [Environment::K8s, Environment::Linux, Environment::Windows] {
            let registry = EmbeddedRegistry::new(env, PathBuf::from("/nonexistent"));
            for (key, definition) in registry.0.iter() {
                assert_eq!(key.to_string(), definition.agent_type_id().to_string());
            }
        }
    }

    #[test]
    fn test_get() {
        let definitions = vec![
            AgentTypeDefinition::empty_with_metadata(
                AgentTypeID::try_from("ns/agent-1:0.0.0").unwrap(),
            ),
            AgentTypeDefinition::empty_with_metadata(
                AgentTypeID::try_from("ns/agent-2:0.0.0").unwrap(),
            ),
        ];

        let registry = EmbeddedRegistry::try_new(definitions.clone()).unwrap();

        let agent_1 = registry
            .get(&AgentTypeID::try_from("ns/agent-1:0.0.0").unwrap())
            .unwrap();
        assert_eq!(definitions[0], agent_1);
        let agent_2 = registry
            .get(&AgentTypeID::try_from("ns/agent-2:0.0.0").unwrap())
            .unwrap();
        assert_eq!(definitions[1], agent_2);

        let err = registry
            .get(&AgentTypeID::try_from("ns/not-existent:0.0.0").unwrap())
            .unwrap_err();
        assert_matches!(err, AgentTypeRegistryError::NotFound(_));
    }

    #[test]
    fn test_insert_duplicate() {
        let mut registry = EmbeddedRegistry::new(Environment::K8s, PathBuf::from("/nonexistent"));

        let definition = AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        );
        let duplicate = definition.clone();

        assert!(registry.insert(definition).is_ok());

        let err = registry.insert(duplicate).unwrap_err();
        assert_matches!(err, AgentTypeRegistryError::AlreadyExists(name) => {
            assert_eq!("ns/agent:0.0.0", name);
        })
    }

    #[test]
    fn test_insert_duplicate_via_dynamic_config() {
        let tmp_dir = tempdir().expect("failed to create local temp dir");
        let path = tmp_dir.path();
        File::create(path.join("agent_type_1"))
            .unwrap()
            .write_all(
                r#"
namespace: ns
name: io.test
version: 0.0.0
platform: kubernetes
variables:
  version:
    type: string
    required: true
    description: "test"
deployment:
  objects: {}
    "#
                .as_bytes(),
            )
            .unwrap();

        File::create(path.join("same_agent_is_overwritten"))
            .unwrap()
            .write_all(
                r#"
namespace: ns
name: io.test
version: 0.0.0
platform: kubernetes
variables:
  different:
    type: string
    required: true
    description: "test"
deployment:
  objects: {}
    "#
                .as_bytes(),
            )
            .unwrap();

        File::create(path.join("main_agent_type_is_overwritten"))
            .unwrap()
            .write_all(
                r#"
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: kubernetes
variables:
  different:
    type: string
    required: true
    description: "test"
deployment:
  objects: {}
    "#
                .as_bytes(),
            )
            .unwrap();

        File::create(path.join("wrong_agent_is_skipped"))
            .unwrap()
            .write_all("asdkjfnad".as_bytes())
            .unwrap();

        File::create(path.join("empty_agent_is_skipped"))
            .unwrap()
            .write_all("".as_bytes())
            .unwrap();

        let registry = EmbeddedRegistry::new(Environment::K8s, path.to_path_buf());

        let variables = registry
            .get(&AgentTypeID::try_from("ns/io.test:0.0.0").unwrap())
            .unwrap()
            .variables
            .0;
        assert!(!variables.contains_key("version"));
        assert!(variables.contains_key("different"));
        assert!(
            registry
                .get(&AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap())
                .unwrap()
                .variables
                .0
                .contains_key("different")
        );
    }
}
