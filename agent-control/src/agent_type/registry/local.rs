mod custom;
mod embedded;

#[cfg(test)]
mod agent_type_validation_tests;

use self::custom::custom_definitions;
use self::embedded::embedded_definitions;
use super::AgentTypeRegistryError;
use crate::agent_type::definition::AgentTypeDefinition;
use crate::agent_type::{agent_type_id::AgentTypeID, registry::AgentTypeRegistry};
use crate::environment::Environment;
use std::{collections::HashMap, path::PathBuf};
use tracing::debug;

/// Keeps [AgentTypeDefinition]s in memory, sourced from the embedded definitions and, optionally,
/// the custom agent types provided in a directory (which take precedence over the embedded ones).
#[derive(Debug, Default)]
pub struct LocalRegistry {
    definitions: HashMap<AgentTypeID, AgentTypeDefinition>,
}

impl AgentTypeRegistry for LocalRegistry {
    /// Returns the agent type definition matching the given id, if it is present.
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        self.definitions
            .get(agent_type_id)
            .cloned()
            .ok_or(AgentTypeRegistryError::NotFound(agent_type_id.to_string()))
    }
}

impl LocalRegistry {
    /// Builds a [LocalRegistry] loading the embedded agent types matching the given [Environment]
    /// and overlaying the custom agent types found in the given directory (custom definitions take
    /// precedence over the embedded ones). Definitions whose environment does not match `env` are
    /// skipped, so the registry only contains entries the running binary can actually use.
    pub fn new(env: Environment, custom_agent_types_path: PathBuf) -> Self {
        let mut registry =
            Self::try_new(embedded_definitions(env)).expect("Conflicting agent type definitions");

        // Custom agent types are dynamic, hence they take precedence over the embedded ones.
        for definition in custom_definitions(custom_agent_types_path)
            .into_iter()
            .filter(|definition| definition.metadata.environment == env)
        {
            debug!("Storing custom agent type: {}", definition.agent_type_id());
            registry
                .definitions
                .insert(definition.agent_type_id().clone(), definition);
        }
        registry
    }

    /// Builds a [LocalRegistry] from an iterator of definitions, failing if two definitions share
    /// the same id.
    fn try_new<T: IntoIterator<Item = AgentTypeDefinition>>(
        definitions_iter: T,
    ) -> Result<Self, AgentTypeRegistryError> {
        let mut registry = Self::default();
        definitions_iter
            .into_iter()
            .try_for_each(|definition| registry.insert_unique(definition))?;
        Ok(registry)
    }

    /// Inserts a definition, returning an [AgentTypeRegistryError::AlreadyExists] error if a
    /// definition with the same id is already present.
    fn insert_unique(
        &mut self,
        definition: AgentTypeDefinition,
    ) -> Result<(), AgentTypeRegistryError> {
        let id = definition.agent_type_id().clone();
        if self.definitions.contains_key(&id) {
            return Err(AgentTypeRegistryError::AlreadyExists(id.to_string()));
        }
        self.definitions.insert(id, definition);
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    impl LocalRegistry {
        /// Builds a [LocalRegistry] with only the embedded agent types for the given
        /// [Environment] (no custom ones).
        pub fn embedded_only(env: Environment) -> Self {
            Self::try_new(embedded_definitions(env)).expect("Conflicting agent type definitions")
        }

        pub fn iter_definitions(&self) -> impl Iterator<Item = &AgentTypeDefinition> {
            self.definitions.values()
        }
    }

    impl From<AgentTypeDefinition> for LocalRegistry {
        fn from(value: AgentTypeDefinition) -> Self {
            let mut registry = Self::default();
            registry.insert_unique(value).unwrap();
            registry
        }
    }

    #[test]
    fn test_default_loads_embedded_definitions() {
        let registry = LocalRegistry::embedded_only(Environment::K8s);

        // The key for each definition should be its agent type id
        for definition in registry.iter_definitions() {
            assert_eq!(
                &registry.get(definition.agent_type_id()).unwrap(),
                definition
            );
        }

        // Loading with a nonexistent custom dir must yield exactly the embedded definitions,
        // not merely the same count.
        let registry_nonexistent_custom =
            LocalRegistry::new(Environment::K8s, PathBuf::from("/nonexistent/path"));
        assert_eq!(
            registry.definitions, registry_nonexistent_custom.definitions,
            "a nonexistent custom dir should leave the registry equal to the embedded one"
        );
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

        let registry = LocalRegistry::try_new(definitions.clone()).unwrap();

        let agent_1 = registry
            .get(&AgentTypeID::try_from("ns/agent-1:0.0.0").unwrap())
            .unwrap();
        assert_eq!(definitions[0], agent_1);
        let agent_2 = registry
            .get(&AgentTypeID::try_from("ns/agent-2:0.0.0").unwrap())
            .unwrap();
        assert_eq!(definitions[1], agent_2);

        assert_matches!(
            registry
                .get(&AgentTypeID::try_from("ns/not-existent:0.0.0").unwrap()),
            Err(AgentTypeRegistryError::NotFound(s)) => {
                assert_eq!(s, "ns/not-existent:0.0.0".to_string());
            }
        );
    }

    #[test]
    fn test_insert_unique_duplicate() {
        let mut registry = LocalRegistry::default();

        let definition = AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        );
        let duplicate = definition.clone();

        assert!(registry.insert_unique(definition).is_ok());

        let err = registry.insert_unique(duplicate).unwrap_err();
        assert_matches!(err, AgentTypeRegistryError::AlreadyExists(name) => {
            assert_eq!("ns/agent:0.0.0", name);
        })
    }

    #[test]
    fn test_new_overlays_custom_over_embedded_and_earlier_custom() {
        // Variables are used here only to tell which definition won the overlay.
        let custom_agent_type = |namespace: &str, name: &str, version: &str, variable: &str| {
            format!(
                r#"
namespace: {namespace}
name: {name}
version: {version}
platform: kubernetes
variables:
  {variable}:
    type: string
    required: true
    description: "test"
deployment:
  objects: {{}}
"#
            )
        };

        let tmp_dir = tempdir().expect("failed to create local temp dir");
        let path = tmp_dir.path();
        // Two custom files for the same id: the later one (by sorted file name) must win.
        write_file(
            path,
            "01_custom",
            &custom_agent_type("ns", "io.test", "0.0.0", "version"),
        );
        write_file(
            path,
            "02_custom",
            &custom_agent_type("ns", "io.test", "0.0.0", "different"),
        );
        // A custom file overriding an embedded agent type.
        write_file(
            path,
            "overrides_embedded",
            &custom_agent_type(
                "newrelic",
                "com.newrelic.infrastructure",
                "0.1.0",
                "different",
            ),
        );

        let registry = LocalRegistry::new(Environment::K8s, path.to_path_buf());

        // Custom over earlier custom: the second file's definition wins.
        let custom = registry
            .get(&AgentTypeID::try_from("ns/io.test:0.0.0").unwrap())
            .unwrap();
        assert!(!custom.variables.0.contains_key("version"));
        assert!(custom.variables.0.contains_key("different"));

        // Custom over embedded: the embedded infrastructure definition is replaced.
        assert!(
            registry
                .get(&AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap())
                .unwrap()
                .variables
                .0
                .contains_key("different")
        );
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        File::create(dir.join(name))
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }
}
