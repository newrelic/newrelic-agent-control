use crate::agent_control::config::SubAgentsMap;
use crate::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use crate::agent_control::run::Environment;
use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use crate::agent_type::definition::{AgentType, AgentTypeDefinition};
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::render::TemplateRenderer;
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::rendered::OnHost;
use crate::agent_type::runtime_config::{Deployment, Runtime, rendered};
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::secret_variables::{
    SecretVariables, SecretVariablesError, load_env_vars,
};
use crate::secrets_provider::SecretsProviders;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::parent_agent_resolver::ParentAgentResolver;
use crate::values::yaml_config::YAMLConfig;

use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    #[error("error assembling agents: {0}")]
    EffectiveAgentsAssemblerError(String),
    #[error("error assembling agents: {0}")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("error assembling agents: {0}")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("error assembling agents: {0}")]
    AgentTypeError(#[from] AgentTypeError),
    #[error("error assembling agents: {0}")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("error loading secrets: {0}")]
    SecretVariablesError(#[from] SecretVariablesError),
}

#[derive(Error, Debug)]
pub enum AgentTypeDefinitionError {
    #[error("invalid agent-type for '{0}' environment: {1}")]
    EnvironmentError(AgentTypeError, Environment),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveAgent {
    agent_identity: AgentIdentity,
    runtime_config: rendered::Runtime,
}

impl Display for EffectiveAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.agent_identity.id.to_string())
    }
}

impl EffectiveAgent {
    pub(crate) fn new(agent_identity: AgentIdentity, runtime_config: rendered::Runtime) -> Self {
        Self {
            agent_identity,
            runtime_config,
        }
    }

    // Depending on the environment this method returns either the linux or windows deployment
    pub(crate) fn get_onhost_config(&self) -> Result<&OnHost, EffectiveAgentsAssemblerError> {
        #[cfg(target_family = "windows")]
        return self.runtime_config.deployment.windows.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing windows deployment configuration".to_string(),
            ),
        );
        #[cfg(target_family = "unix")]
        self.runtime_config.deployment.linux.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing linux deployment configuration".to_string(),
            ),
        )
    }

    pub(crate) fn get_k8s_config(&self) -> Result<&K8s, EffectiveAgentsAssemblerError> {
        self.runtime_config.deployment.k8s.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing k8s deployment configuration".to_string(),
            ),
        )
    }

    pub(crate) fn get_agent_identity(&self) -> &AgentIdentity {
        &self.agent_identity
    }
}

impl TryFrom<EffectiveAgent> for K8s {
    type Error = EffectiveAgentsAssemblerError;

    fn try_from(value: EffectiveAgent) -> Result<Self, Self::Error> {
        value.runtime_config.deployment.k8s.ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing k8s deployment configuration".to_string(),
            ),
        )
    }
}

pub trait EffectiveAgentsAssembler {
    /// Assemble an [EffectiveAgent] from an [AgentIdentity]. The implementer is responsible for
    /// getting the AgentType and all needed values to render the Runtime config.
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        yaml_config: YAMLConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

/// Implements [EffectiveAgentsAssembler] and is responsible for:
/// - Getting [AgentType] from [AgentRegistry]
/// - Getting Local or Remote configs from [ConfigRepository]
/// - Rendering the [Runtime] configuration of an Agent
///
/// Important: Assembling an Agent may mutate the state of external resources by creating
/// or removing configs when the Runtime is [Renderer].
pub struct LocalEffectiveAgentsAssembler<R, P>
where
    R: AgentRegistry,
    P: ParentAgentResolver,
{
    registry: Arc<R>,
    renderer: TemplateRenderer,
    variable_constraints: VariableConstraints,
    secrets_providers: SecretsProviders,
    remote_dir: PathBuf,
    parent_agent_resolver: Arc<P>,
    agents_map: Arc<RwLock<SubAgentsMap>>,
}

impl<R, P> LocalEffectiveAgentsAssembler<R, P>
where
    R: AgentRegistry,
    P: ParentAgentResolver,
{
    pub fn new(
        registry: Arc<R>,
        renderer: TemplateRenderer,
        variable_constraints: VariableConstraints,
        secrets_providers: SecretsProviders,
        remote_dir: &Path,
        parent_agent_resolver: Arc<P>,
        agents_map: Arc<RwLock<SubAgentsMap>>,
    ) -> Self {
        LocalEffectiveAgentsAssembler {
            registry,
            renderer,
            variable_constraints,
            secrets_providers,
            remote_dir: remote_dir.to_path_buf(),
            parent_agent_resolver,
            agents_map,
        }
    }
}

impl<R, P> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, P>
where
    R: AgentRegistry,
    P: ParentAgentResolver,
{
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        values: YAMLConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the agent type definition
        let agent_type_definition = self
            .registry
            .get(&agent_identity.agent_type_id.to_string())?;
        // Build the corresponding agent type
        let agent_type = build_agent_type(
            agent_type_definition,
            environment,
            &self.variable_constraints,
        )?;

        // Build the agent attributes
        let attributes =
            AgentAttributes::try_new(agent_identity.id.to_owned(), self.remote_dir.to_path_buf())
                .map_err(|e| {
                EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(e.to_string())
            })?;

        // Resolve parent agent IDs for post-install hook expansion
        let (parent_agent_vars, parent_agent_ids) = if let Some(parent_agent_type) = agent_identity.agent_type_id.parent_agent() {
            let agents_map = self.agents_map.read()
                .map_err(|e| EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                    format!("Failed to acquire read lock on agents map: {}", e)
                ))?;

            let parent_ids = self.parent_agent_resolver.resolve_parent_agent_ids(parent_agent_type, &agents_map);

            if parent_ids.is_empty() {
                // Validation should have caught this, but handle gracefully
                return Err(EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                    format!("Parent agent '{}' not found for agent '{}'", parent_agent_type, agent_identity.id)
                ));
            }

            // Use any parent for initial template resolution (required for variable substitution).
            // All parents will be handled equally during hook expansion below.
            let template_parent_id = &parent_ids[0];
            let vars = AgentAttributes::parent_agent_variables(template_parent_id, &self.remote_dir)
                .map_err(|e| EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(e.to_string()))?;

            (vars, parent_ids)
        } else {
            (HashMap::new(), Vec::new())
        };

        // Values are expanded substituting all ${nr-env...} with environment variables.
        // Notice that only environment variables are taken into consideration (no other vars for example)
        let secret_variables = SecretVariables::try_from(values.clone())?;
        let env_vars = load_env_vars();
        let secrets = secret_variables.load_secrets(&self.secrets_providers)?;

        let mut runtime_config = self
            .renderer
            .render(agent_type, values, attributes, env_vars, secrets, parent_agent_vars)?;

        // Expand post-install hooks for ALL parent agents (even if just one).
        // This ensures each parent agent gets its own set of hooks with correctly resolved paths.
        if !parent_agent_ids.is_empty() {
            runtime_config = self.expand_post_install_hooks_for_parents(runtime_config, &parent_agent_ids)?;
        }

        Ok(EffectiveAgent::new(agent_identity.clone(), runtime_config))
    }
}

impl<R, P> LocalEffectiveAgentsAssembler<R, P>
where
    R: AgentRegistry,
    P: ParentAgentResolver,
{
    /// Expands post-install hooks to create one hook per parent agent instance.
    /// Each parent agent gets its own set of hooks with parent-specific paths properly resolved.
    /// This ensures all parent agents are treated equally, regardless of how many exist.
    fn expand_post_install_hooks_for_parents(
        &self,
        mut runtime_config: rendered::Runtime,
        parent_agent_ids: &[crate::agent_control::agent_id::AgentID],
    ) -> Result<rendered::Runtime, EffectiveAgentsAssemblerError> {
        use crate::agent_type::runtime_config::on_host::package::rendered::{PostInstallAction, PostInstallHook};
        use crate::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;

        // Expand hooks for Linux deployment
        if let Some(ref mut linux) = runtime_config.deployment.linux {
            for (_package_id, package) in &mut linux.packages {
                let original_hooks = std::mem::take(&mut package.post_install);
                let mut expanded_hooks = Vec::new();

                for hook in original_hooks {
                    // For each parent agent, create a copy of the hook with parent-specific paths
                    for parent_id in parent_agent_ids {
                        let parent_fs_dir = self.remote_dir
                            .join(AGENT_FILESYSTEM_FOLDER_NAME)
                            .join(&parent_id.to_string());

                        let expanded_action = match &hook.action {
                            PostInstallAction::Copy { source, destination, create_parent_dirs } => {
                                // Replace parent-specific path in destination
                                let new_dest = self.replace_parent_path_in_hook(destination, &parent_fs_dir);
                                PostInstallAction::Copy {
                                    source: source.clone(),
                                    destination: new_dest,
                                    create_parent_dirs: *create_parent_dirs,
                                }
                            }
                            PostInstallAction::Symlink { source, destination, create_parent_dirs } => {
                                let new_dest = self.replace_parent_path_in_hook(destination, &parent_fs_dir);
                                PostInstallAction::Symlink {
                                    source: source.clone(),
                                    destination: new_dest,
                                    create_parent_dirs: *create_parent_dirs,
                                }
                            }
                        };

                        expanded_hooks.push(PostInstallHook {
                            action: expanded_action,
                        });
                    }
                }

                package.post_install = expanded_hooks;
            }
        }

        // Expand hooks for Windows deployment
        if let Some(ref mut windows) = runtime_config.deployment.windows {
            for (_package_id, package) in &mut windows.packages {
                let original_hooks = std::mem::take(&mut package.post_install);
                let mut expanded_hooks = Vec::new();

                for hook in original_hooks {
                    for parent_id in parent_agent_ids {
                        let parent_fs_dir = self.remote_dir
                            .join(AGENT_FILESYSTEM_FOLDER_NAME)
                            .join(&parent_id.to_string());

                        let expanded_action = match &hook.action {
                            PostInstallAction::Copy { source, destination, create_parent_dirs } => {
                                let new_dest = self.replace_parent_path_in_hook(destination, &parent_fs_dir);
                                PostInstallAction::Copy {
                                    source: source.clone(),
                                    destination: new_dest,
                                    create_parent_dirs: *create_parent_dirs,
                                }
                            }
                            PostInstallAction::Symlink { source, destination, create_parent_dirs } => {
                                let new_dest = self.replace_parent_path_in_hook(destination, &parent_fs_dir);
                                PostInstallAction::Symlink {
                                    source: source.clone(),
                                    destination: new_dest,
                                    create_parent_dirs: *create_parent_dirs,
                                }
                            }
                        };

                        expanded_hooks.push(PostInstallHook {
                            action: expanded_action,
                        });
                    }
                }

                package.post_install = expanded_hooks;
            }
        }

        Ok(runtime_config)
    }

    /// Replaces the parent filesystem directory in a hook destination path.
    /// During initial templating, one parent's variables are used for variable substitution.
    /// This method replaces that templated path with each specific parent's filesystem directory.
    fn replace_parent_path_in_hook(&self, original: &PathBuf, new_parent_fs_dir: &PathBuf) -> PathBuf {
        // The original path was templated during the render phase.
        // We detect the filesystem directory pattern and replace it with each parent's specific dir.
        let original_str = original.to_string_lossy();

        // Look for the filesystem agent directory pattern
        if let Some(idx) = original_str.find(AGENT_FILESYSTEM_FOLDER_NAME) {
            // Extract the part after filesystem/{agent_id}/
            let after_fs = &original_str[idx + AGENT_FILESYSTEM_FOLDER_NAME.len()..];
            if let Some(slash_idx) = after_fs.find('/').or_else(|| after_fs.find('\\')) {
                let relative_path = &after_fs[slash_idx + 1..];
                return new_parent_fs_dir.join(relative_path);
            }
        }

        // Fallback: just use the original path (shouldn't happen in normal cases)
        original.clone()
    }
}

/// Builds an [AgentType] given the provided [AgentTypeDefinition] and environment.
pub fn build_agent_type(
    definition: AgentTypeDefinition,
    environment: &Environment,
    variable_constraints: &VariableConstraints,
) -> Result<AgentType, AgentTypeDefinitionError> {
    // Select vars and runtime config according to the environment
    let (specific_vars, runtime_config) = match environment {
        Environment::K8s => (
            definition.variables.k8s,
            Runtime {
                deployment: Deployment {
                    linux: None,
                    windows: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
        Environment::Linux => (
            definition.variables.linux,
            Runtime {
                deployment: Deployment {
                    k8s: None,
                    windows: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
        Environment::Windows => (
            definition.variables.windows,
            Runtime {
                deployment: Deployment {
                    k8s: None,
                    linux: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
    };
    // Merge common and specific variables
    let merged_variables = definition
        .variables
        .common
        .merge(specific_vars)
        .map_err(|err| AgentTypeDefinitionError::EnvironmentError(err, *environment))?;

    let agent_type_vars = merged_variables.with_config(variable_constraints);

    Ok(AgentType::new(
        definition.agent_type_id,
        agent_type_vars,
        runtime_config,
    ))
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {

    use super::*;
    use crate::agent_control::{agent_id::AgentID, run::on_host::AGENT_CONTROL_MODE_ON_HOST};
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistry;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    mock! {
        pub EffectiveAgentAssembler {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssembler {
            fn assemble_agent(
                &self,
                agent_identity:&AgentIdentity,
                yaml_config: YAMLConfig,
                environment: &Environment,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;

        }
    }

    impl MockEffectiveAgentAssembler {
        pub fn should_assemble_agent(
            &mut self,
            agent_identity: &AgentIdentity,
            yaml_config: &YAMLConfig,
            environment: &Environment,
            effective_agent: EffectiveAgent,
            times: usize,
        ) {
            self.expect_assemble_agent()
                .times(times)
                .with(
                    predicate::eq(agent_identity.clone()),
                    predicate::eq(yaml_config.clone()),
                    predicate::eq(*environment),
                )
                .returning(move |_, _, _| Ok(effective_agent.clone()));
        }
    }

    impl<R, P> LocalEffectiveAgentsAssembler<R, P>
    where
        R: AgentRegistry,
        P: ParentAgentResolver,
    {
        pub fn new_for_testing(registry: R, parent_resolver: P) -> Self {
            Self {
                registry: Arc::new(registry),
                renderer: TemplateRenderer::default(),
                variable_constraints: VariableConstraints::default(),
                secrets_providers: SecretsProviders::default(),
                remote_dir: PathBuf::default(),
                parent_agent_resolver: Arc::new(parent_resolver),
                agents_map: Arc::new(RwLock::new(HashMap::new())),
            }
        }
    }

    #[test]
    fn test_assemble_agents() {
        // Mocks
        let mut registry = MockAgentRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
        ));
        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());
        let values = YAMLConfig::default();

        //Expectations
        registry.should_get("ns/name:0.0.1".to_string(), &agent_type_definition);

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry, crate::sub_agent::parent_agent_resolver::DefaultParentAgentResolver);

        let effective_agent = assembler
            .assemble_agent(&agent_identity, values, &AGENT_CONTROL_MODE_ON_HOST)
            .unwrap();

        assert_eq!(agent_identity, effective_agent.agent_identity);
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
        ));

        //Expectations
        registry.should_not_get("namespace/name:0.0.1".to_string());
        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry, crate::sub_agent::parent_agent_resolver::DefaultParentAgentResolver);

        let result = assembler.assemble_agent(
            &agent_identity,
            YAMLConfig::default(),
            &AGENT_CONTROL_MODE_ON_HOST,
        );

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: agent type namespace/name:0.0.1 not found",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_build_agent_type() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(AGENT_TYPE_DEFINITION).unwrap();

        let k8s_agent_type = build_agent_type(
            definition.clone(),
            &Environment::K8s,
            &VariableConstraints::default(),
        )
        .unwrap();
        let k8s_vars = k8s_agent_type.variables.flatten();
        assert!(k8s_vars.contains_key("config.really_common"));
        let var = k8s_vars.get("config.var").unwrap();
        assert_eq!("K8s var".to_string(), var.description);
        assert!(
            k8s_agent_type.runtime_config.deployment.linux.is_none(),
            "linux deployment for k8s should be none"
        );
        assert!(
            k8s_agent_type.runtime_config.deployment.windows.is_none(),
            "windows deployment for k8s should be none"
        );

        let on_host_agent_type = build_agent_type(
            definition,
            &AGENT_CONTROL_MODE_ON_HOST,
            &VariableConstraints::default(),
        )
        .unwrap();
        let on_host_vars = on_host_agent_type.variables.flatten();
        assert!(on_host_vars.contains_key("config.really_common"));
        let var = on_host_vars.get("config.var").unwrap();
        #[cfg(target_family = "unix")]
        assert_eq!("Linux var".to_string(), var.description);
        #[cfg(target_family = "windows")]
        assert_eq!("Windows var".to_string(), var.description);
        assert!(
            on_host_agent_type.runtime_config.deployment.k8s.is_none(),
            "K8s deployment for on_host should be none"
        );
    }

    #[test]
    fn test_build_agent_type_error() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(CONFLICTING_AGENT_TYPE_DEFINITION).unwrap();

        let expected_err = build_agent_type(
            definition,
            &Environment::K8s,
            &VariableConstraints::default(),
        )
        .err()
        .unwrap();
        assert_matches!(expected_err, AgentTypeDefinitionError::EnvironmentError(err, env) => {
            assert_matches!(err, AgentTypeError::ConflictingVariableDefinition(key) => {
                assert_eq!("config.var".to_string(), key);
            });
            assert_matches!(env, Environment::K8s);
        });
    }

    const AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
variables:
  common:
    config:
      really_common:
        description: "Common var"
        type: string
        required: true
  k8s:
    config:
      var:
        description: "K8s var"
        type: string
        required: true
  linux:
    config:
      var:
        description: "Linux var"
        type: string
        required: true
  windows:
    config:
      var:
        description: "Windows var"
        type: string
        required: true
deployment:
    linux:
      executables:
        - id: my-exec
          path: /some/path
          args: 
            - ${nr-var:config.really_common} 
            - ${config.var}
    windows:
      executables:
        - id: my-exec
          path: /some/path
          args: 
            - ${nr-var:config.really_common} 
            - ${config.var}
    k8s:
      objects:
        chart:
          apiVersion: some.api.version/v1
          kind: SomeKind
          metadata:
            name: ${nr-sub:agent_id}
            namespace: ${nr-ac:namespace}
          spec:
            some_key: ${nr-var:config.really_common}
            other: ${nr-avar:config.var}
"#;

    const CONFLICTING_AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
variables:
  common:
    config:
      var:
        description: "Common variable"
        type: string
        required: true
  k8s:
    config:
      var:
        description: "K8s variable"
        type: string
        required: true
deployment:
    k8s:
      objects: {}
"#;
}
