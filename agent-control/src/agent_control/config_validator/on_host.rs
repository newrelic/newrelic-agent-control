//! On-host dynamic config validation enforcing the single-owner rule for shared-filesystem paths.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::definition::AgentTypeDefinition;
use crate::agent_type::registry::AgentTypeRegistry;
use crate::agent_type::runtime_config::Deployment;
use crate::agent_type::runtime_config::on_host::filesystem::DeclaredPaths;

use super::{DynamicConfigValidator, DynamicConfigValidatorError};

/// The shared-filesystem paths a single agent declares, tagged with its identity for reporting.
struct AgentSharedPaths {
    agent_id: AgentID,
    agent_type: AgentTypeID,
    claimed: Vec<PathBuf>,
}

impl AgentSharedPaths {
    fn from_definition(
        agent_id: AgentID,
        agent_type: AgentTypeID,
        definition: &AgentTypeDefinition,
    ) -> Self {
        let declared = match &definition.runtime_config.deployment {
            // Setting an empty path because we are comparing paths relative to the shared-fs paths which is not
            // relevant for potential error messages.
            Deployment::Host(on_host) => on_host.shared_filesystem().declared_paths(Path::new("")),
            // K8s agent-types are not expected on on-host validation
            Deployment::K8s(_) => DeclaredPaths::default(),
        };
        let claimed: Vec<PathBuf> = declared
            .files
            .into_iter()
            .chain(declared.managed_dirs)
            .collect();
        Self {
            agent_id,
            agent_type,
            claimed,
        }
    }

    /// Returns the first pair of claimed paths (one from each agent) that overlap, if any.
    /// Check [SharedFilesystemPathValidator] docs for details.
    fn overlapping_path<'a>(&self, other: &'a Self) -> Option<(&Path, &'a Path)> {
        self.claimed.iter().find_map(|own| {
            other
                .claimed
                .iter()
                .find(|other| own.starts_with(other) || other.starts_with(own))
                .map(|other| (own.as_path(), other.as_path()))
        })
    }
}

/// Enforces the single-owner rule for the on-host shared filesystem. For files declared by two different agents:
/// * Two files with the same path overlap
/// * A declared directory and a child file overlap
/// * Two siblings files in the same undeclared directory do not overlap
///
/// Shared paths are static (the Agent Type entry keys), so this reads them straight from the
/// registry without rendering. Resolving each agent type also surfaces unknown-type errors, therefore
/// the [RegistryDynamicConfigValidator](super::RegistryDynamicConfigValidator) check is addressed by this validator.
pub struct SharedFilesystemPathValidator<R: AgentTypeRegistry> {
    registry: Arc<R>,
}

impl<R: AgentTypeRegistry> SharedFilesystemPathValidator<R> {
    /// Builds a validator resolving agent types through `registry` to read their declared shared paths.
    pub fn new(registry: Arc<R>) -> Self {
        Self { registry }
    }
}

impl<R> DynamicConfigValidator for SharedFilesystemPathValidator<R>
where
    R: AgentTypeRegistry,
{
    fn validate(
        &self,
        dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<(), DynamicConfigValidatorError> {
        let mut claims = Vec::with_capacity(dynamic_config.agents.len());
        for (agent_id, sub_agent_cfg) in &dynamic_config.agents {
            let definition = self
                .registry
                .get(&sub_agent_cfg.agent_type)
                .map_err(|err| {
                    DynamicConfigValidatorError(format!("AgentType registry check failed: {err}"))
                })?;
            claims.push(AgentSharedPaths::from_definition(
                agent_id.clone(),
                sub_agent_cfg.agent_type.clone(),
                &definition,
            ));
        }

        reject_shared_path_conflicts(claims)
    }
}

fn reject_shared_path_conflicts(
    claims: Vec<AgentSharedPaths>,
) -> Result<(), DynamicConfigValidatorError> {
    for (index, first) in claims.iter().enumerate() {
        for second in &claims[index + 1..] {
            if let Some((first_path, second_path)) = first.overlapping_path(second) {
                return Err(DynamicConfigValidatorError(shared_conflict_message(
                    first,
                    second,
                    first_path,
                    second_path,
                )));
            }
        }
    }
    Ok(())
}

/// Builds the error describing a shared-filesystem conflict between two agents.
fn shared_conflict_message(
    first: &AgentSharedPaths,
    second: &AgentSharedPaths,
    first_path: &Path,
    second_path: &Path,
) -> String {
    let overlap = if first_path == second_path {
        format!(
            "both declare shared filesystem path `{}`",
            first_path.display()
        )
    } else {
        format!(
            "declare overlapping shared filesystem paths `{}` and `{}`",
            first_path.display(),
            second_path.display()
        )
    };
    format!(
        "shared filesystem conflict: agents `{}` ({}) and `{}` ({}) {overlap}",
        first.agent_id, first.agent_type, second.agent_id, second.agent_type
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::SubAgentConfig;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;

    /// Builds a host Agent Type from a `shared_filesystem` entry block. The block is parsed and
    /// re-serialized, so tests can write it at natural (column-zero) indentation without aligning
    /// it under `deployment.shared_filesystem`.
    fn host_type_with_shared(
        namespace: &str,
        name: &str,
        shared_filesystem: &str,
    ) -> AgentTypeDefinition {
        let shared_filesystem: serde_json::Value = serde_saphyr::from_str(shared_filesystem)
            .expect("shared_filesystem entries must parse");
        let definition = serde_json::json!({
            "name": name,
            "namespace": namespace,
            "version": "0.0.1",
            "platform": "host",
            "operating_system": "linux",
            "variables": {},
            "deployment": { "shared_filesystem": shared_filesystem },
        });
        let yaml = serde_saphyr::to_string(&definition).expect("definition must serialize");
        serde_saphyr::from_str(&yaml)
            .unwrap_or_else(|e| panic!("host agent type must parse: {e}\n---\n{yaml}"))
    }

    /// Builds a dynamic config from `(agent_id, agent_type)` pairs.
    fn dynamic_config(agents: &[(&str, &str)]) -> AgentControlDynamicConfig {
        let agents = agents
            .iter()
            .map(|(agent_id, agent_type)| {
                (
                    AgentID::try_from(*agent_id).unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeID::try_from(*agent_type).unwrap(),
                    },
                )
            })
            .collect();
        AgentControlDynamicConfig {
            agents,
            ..Default::default()
        }
    }

    fn validator(
        registry: MockAgentTypeRegistry,
    ) -> SharedFilesystemPathValidator<MockAgentTypeRegistry> {
        SharedFilesystemPathValidator::new(Arc::new(registry))
    }

    /// Two agents dropping distinct files into the same co-owned directory do not conflict.
    #[test]
    fn distinct_shared_files_across_agents_are_allowed() {
        let redis = host_type_with_shared(
            "test",
            "redis",
            r#"
ohi-configs:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: "integration: redis"
"#,
        );
        let mysql = host_type_with_shared(
            "test",
            "mysql",
            r#"
ohi-configs:
  kind: dir
  entries:
    nri-mysql.yaml:
      kind: file
      text: "integration: mysql"
"#,
        );

        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(AgentTypeID::try_from("test/redis:0.0.1").unwrap(), &redis);
        registry.should_get(AgentTypeID::try_from("test/mysql:0.0.1").unwrap(), &mysql);

        let config = dynamic_config(&[
            ("redis-agent", "test/redis:0.0.1"),
            ("mysql-agent", "test/mysql:0.0.1"),
        ]);

        assert!(validator(registry).validate(&config).is_ok());
    }

    #[test]
    fn files_with_extension_prefix_overlap_are_allowed() {
        let short = host_type_with_shared(
            "test",
            "short",
            r#"
some_path:
  kind: dir
  entries:
    some_file.txt:
      kind: file
      text: short
"#,
        );
        let long = host_type_with_shared(
            "test",
            "long",
            r#"
some_path:
  kind: dir
  entries:
    some_file.txt.txt:
      kind: file
      text: long
"#,
        );

        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(AgentTypeID::try_from("test/short:0.0.1").unwrap(), &short);
        registry.should_get(AgentTypeID::try_from("test/long:0.0.1").unwrap(), &long);

        let config = dynamic_config(&[
            ("short-agent", "test/short:0.0.1"),
            ("long-agent", "test/long:0.0.1"),
        ]);

        assert!(
            validator(registry).validate(&config).is_ok(),
            "`some_file.txt` and `some_file.txt.txt` are distinct files and must not conflict"
        );
    }

    #[test]
    fn managed_dir_and_prefixed_sibling_file_are_allowed() {
        let managed = host_type_with_shared(
            "test",
            "managed",
            r#"
some_dir:
  kind: dir_content_from_map
  source: ${nr-var:logging}
"#,
        );
        let sibling = host_type_with_shared(
            "test",
            "sibling",
            r#"
some_dir.txt:
  kind: file
  text: hi
"#,
        );

        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(
            AgentTypeID::try_from("test/managed:0.0.1").unwrap(),
            &managed,
        );
        registry.should_get(
            AgentTypeID::try_from("test/sibling:0.0.1").unwrap(),
            &sibling,
        );

        let config = dynamic_config(&[
            ("managed-agent", "test/managed:0.0.1"),
            ("sibling-agent", "test/sibling:0.0.1"),
        ]);

        assert!(
            validator(registry).validate(&config).is_ok(),
            "`some_dir` (managed) and `some_dir.txt` are distinct paths and must not conflict"
        );
    }

    #[test]
    fn two_instances_of_same_type_declaring_shared_fs_conflict() {
        let redis = host_type_with_shared(
            "test",
            "redis",
            r#"
ohi-configs:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: "integration: redis"
"#,
        );

        let mut registry = MockAgentTypeRegistry::new();
        let redis_id = AgentTypeID::try_from("test/redis:0.0.1").unwrap();
        // The same type is resolved once per configured agent.
        registry.should_get(redis_id.clone(), &redis);
        registry.should_get(redis_id, &redis);

        let config = dynamic_config(&[
            ("redis-agent", "test/redis:0.0.1"),
            ("redis-agent-2", "test/redis:0.0.1"),
        ]);

        let err = validator(registry)
            .validate(&config)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("redis-agent"),
            "error should name the agents: {err}"
        );
        assert!(
            err.contains("redis-agent-2"),
            "error should name the agents: {err}"
        );
        assert!(
            err.contains("nri-redis.yaml"),
            "error should name the conflicting path: {err}"
        );
    }

    /// A file one agent drops into a directory that another agent owns wholesale
    /// (`dir_content_from_map`) is a conflict.
    #[test]
    fn file_inside_another_agents_managed_dir_conflicts() {
        let managed = host_type_with_shared(
            "test",
            "logsd",
            r#"
logs.d:
  kind: dir_content_from_map
  source: ${nr-var:logging}
"#,
        );
        let dropper = host_type_with_shared(
            "test",
            "dropper",
            r#"
logs.d:
  kind: dir
  entries:
    extra.yaml:
      kind: file
      text: hi
"#,
        );

        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(AgentTypeID::try_from("test/logsd:0.0.1").unwrap(), &managed);
        registry.should_get(
            AgentTypeID::try_from("test/dropper:0.0.1").unwrap(),
            &dropper,
        );

        let config = dynamic_config(&[
            ("logsd-agent", "test/logsd:0.0.1"),
            ("dropper-agent", "test/dropper:0.0.1"),
        ]);

        assert!(validator(registry).validate(&config).is_err());
    }

    #[test]
    fn unknown_agent_type_is_rejected() {
        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get_not_found(AgentTypeID::try_from("test/missing:0.0.1").unwrap());

        let config = dynamic_config(&[("some-agent", "test/missing:0.0.1")]);

        assert!(validator(registry).validate(&config).is_err());
    }
}
