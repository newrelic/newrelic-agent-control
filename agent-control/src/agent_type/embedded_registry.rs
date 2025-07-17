use super::{
    agent_type_registry::{AgentRegistry, AgentRepositoryError},
    definition::AgentTypeDefinition,
};
use std::{collections::HashMap, fs, path::PathBuf};
use tracing::{debug, error};

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
        // Loading the static agentTypes
        let mut registry =
            Self::try_new(Self::definitions()).expect("Conflicting agent type definitions");

        // Loading, if any, the dynamic agent types from the directory.
        // Since they are dynamic, they are taking the precedence over the static ones.
        Self::dynamic_agent_type(dynamic_agent_type_path)
            .iter()
            .for_each(|agent_type| {
                let metadata = agent_type.agent_type_id.to_string();
                debug!("Storing dynamic agent type: {}", metadata);
                registry.0.insert(metadata, agent_type.clone());
            });
        registry
    }
}

impl AgentRegistry for EmbeddedRegistry {
    fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError> {
        self.0
            .get(name)
            .cloned()
            .ok_or(AgentRepositoryError::NotFound(name.to_string()))
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
        let metadata = definition.agent_type_id.to_string();
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

    /// Read and return the dynamic agent types, if there is an error reading or deserializing it, logs the error.
    fn dynamic_agent_type(path: PathBuf) -> Vec<AgentTypeDefinition> {
        let Ok(entries) = fs::read_dir(path.clone()).inspect_err(
            |err| debug!(error = %err, "Failed reading Dynamic agent types directory {path:?}"),
        ) else {
            return vec![];
        };

        entries
            .flatten()
            .flat_map(|entry| {
                let file = entry.path();
                fs::read(file.clone())
                    .inspect_err(|e| debug!(error = %e, "Skipping file: {file:?}"))
                    .ok()
                    .and_then(|content| {
                        debug!("Loading Dynamic Agent Type: {file:?}");
                        serde_yaml::from_slice::<AgentTypeDefinition>(content.as_slice())
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

    const AGENT_TYPE_AMOUNT: usize = 12;

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
            assert_eq!(key.to_string(), definition.agent_type_id.to_string())
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
            AgentTypeDefinition::empty_with_metadata(
                AgentTypeID::try_from("ns/agent-1:0.0.0").unwrap(),
            ),
            AgentTypeDefinition::empty_with_metadata(
                AgentTypeID::try_from("ns/agent-2:0.0.0").unwrap(),
            ),
        ];

        let registry = EmbeddedRegistry::try_new(definitions.clone()).unwrap();

        let agent_1 = registry.get("ns/agent-1:0.0.0").unwrap();
        assert_eq!(definitions[0], agent_1);
        let agent_2 = registry.get("ns/agent-2:0.0.0").unwrap();
        assert_eq!(definitions[1], agent_2);

        let err = registry.get("not-existent").unwrap_err();
        assert_matches!(err, AgentRepositoryError::NotFound(_));
    }

    #[test]
    fn test_insert_duplicate() {
        let mut registry = EmbeddedRegistry::default();

        let definition = AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        );
        let duplicate = definition.clone();

        assert!(registry.insert(definition).is_ok());

        let err = registry.insert(duplicate).unwrap_err();
        assert_matches!(err, AgentRepositoryError::AlreadyExists(name) => {
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
variables:
  k8s:
    version:
      type: string
      required: true
      description: "test"
deployment:
  k8s:
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
variables:
  k8s:
    different:
      type: string
      required: true
      description: "test"
deployment:
  k8s:
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
variables:
  k8s:
    different:
      type: string
      required: true
      description: "test"
deployment:
  k8s:
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

        let registry = EmbeddedRegistry::new(path.to_path_buf());

        let variables = registry.get("ns/io.test:0.0.0").unwrap().variables.k8s.0;
        assert!(!variables.contains_key("version"));
        assert!(variables.contains_key("different"));
        assert!(
            registry
                .get("newrelic/com.newrelic.infrastructure:0.1.0")
                .unwrap()
                .variables
                .k8s
                .0
                .contains_key("different")
        );
    }
}
