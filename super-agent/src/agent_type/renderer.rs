use std::{collections::HashMap, path::PathBuf};

use crate::{
    sub_agent::persister::config_persister::ConfigurationPersister,
    super_agent::{
        config::AgentID,
        defaults::{GENERATED_FOLDER_NAME, SUPER_AGENT_DATA_DIR},
    },
};

use super::{
    agent_attributes::AgentAttributes,
    agent_values::AgentValues,
    definition::AgentType,
    error::AgentTypeError,
    runtime_config::Runtime,
    runtime_config_templates::Templateable,
    variable::{definition::VariableDefinition, namespace::Namespace},
};

/// Defines how to render an AgentType and obtain the runtime configuration needed to execute a sub agent.
pub trait Renderer {
    /// Renders the runtime configuration in an [AgentType] using the provided values and attributes.
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: AgentValues,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError>;
}

pub struct TemplateRenderer<C: ConfigurationPersister> {
    persister: Option<C>,
    config_base_dir: String,
}

impl<C: ConfigurationPersister> Renderer for TemplateRenderer<C> {
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: AgentValues,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError> {
        // Get empty variables and runtime_config from the agent-type
        let (variables, runtime_config) = (agent_type.variables, agent_type.runtime_config);
        // Fill agent variables
        // `filled_variables` needs to be mutable, in case there are `File` or `MapStringFile` variables, whose path
        // needs to be expanded, checkout out the TODO below for details.
        let mut filled_variables = variables.fill_with_values(values)?.flatten();
        Self::check_all_vars_are_populated(&filled_variables)?;

        // TODO: the persister performs specific actions for file and `File` and `MapStringFile` variables kind only.
        // If another kind with specific actions is introduced, the kind definition should be refactored to allow
        // performing additional actions when filling variables with values.
        if let Some(persister) = &self.persister {
            let sub_agent_config_path = self.subagent_config_path(agent_id);
            filled_variables =
                Self::extend_variables_file_path(sub_agent_config_path, filled_variables);
            persister.delete_agent_config(agent_id)?;
            persister.persist_agent_config(agent_id, &filled_variables)?;
        }

        // Setup namespaced variables
        let ns_variables = Self::build_namespaced_variables(filled_variables, &attributes);
        // Render runtime config
        let rendered_runtime_config = runtime_config.template_with(&ns_variables)?;

        Ok(rendered_runtime_config)
    }
}

impl<C: ConfigurationPersister> Default for TemplateRenderer<C> {
    fn default() -> Self {
        Self {
            persister: None,
            config_base_dir: SUPER_AGENT_DATA_DIR.to_string(),
        }
    }
}

impl<C: ConfigurationPersister> TemplateRenderer<C> {
    pub fn with_config_persister(self, c: C) -> Self {
        Self {
            persister: Some(c),
            ..self
        }
    }

    #[cfg(feature = "custom-local-path")]
    /// Returns a [TemplateRenderer] whose `config_base_dir has the provided `base_dir` prepended.
    pub fn with_base_dir(self, base_dir: &str) -> Self {
        Self {
            config_base_dir: format!("{}{}", base_dir, SUPER_AGENT_DATA_DIR),
            ..self
        }
    }

    // Returns the config path for a sub-agent.
    fn subagent_config_path(&self, agent_id: &AgentID) -> PathBuf {
        PathBuf::from(format!(
            "{}/{}/{}",
            self.config_base_dir, GENERATED_FOLDER_NAME, agent_id
        ))
    }

    // Extends the path of all variables with the sub-agent generated config path.
    fn extend_variables_file_path(
        config_path: PathBuf,
        mut variables: HashMap<String, VariableDefinition>,
    ) -> HashMap<String, VariableDefinition> {
        for var in variables.values_mut() {
            var.extend_file_path(config_path.as_path());
        }
        variables
    }

    fn check_all_vars_are_populated(
        variables: &HashMap<String, VariableDefinition>,
    ) -> Result<(), AgentTypeError> {
        let not_populated = variables
            .clone()
            .into_iter()
            .filter_map(|(k, endspec)| endspec.get_final_value().is_none().then_some(k))
            .collect::<Vec<_>>();
        if !not_populated.is_empty() {
            return Err(AgentTypeError::ValuesNotPopulated(not_populated));
        }
        Ok(())
    }

    fn build_namespaced_variables(
        variables: HashMap<String, VariableDefinition>,
        attributes: &AgentAttributes,
    ) -> HashMap<String, VariableDefinition> {
        // Set the namespaced name to variables
        let vars_iter = variables
            .into_iter()
            .map(|(name, var)| (Namespace::Variable.namespaced_name(&name), var));
        // Get the namespaced variables from sub-agent attributes
        let sub_agent_vars_iter = attributes.sub_agent_variables().into_iter();
        // Join all variables together
        vars_iter
            .chain(sub_agent_vars_iter)
            .collect::<HashMap<String, VariableDefinition>>()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    use fs::directory_manager::DirectoryManagementError;

    use crate::{
        agent_type::{
            definition::AgentType,
            environment::Environment,
            restart_policy::{
                BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
            },
            runtime_config::Args,
        },
        sub_agent::persister::{
            config_persister::{test::MockConfigurationPersisterMock, PersistError},
            config_persister_file::ConfigurationPersisterFile,
        },
    };

    use super::*;

    mock! {
         pub(crate) RendererMock {}

         impl Renderer for RendererMock {
             fn render(
                &self,
                agent_id: &AgentID,
                agent_type: AgentType,
                values: AgentValues,
                attributes: AgentAttributes,
            ) -> Result<Runtime, AgentTypeError>;
         }
    }

    impl MockRendererMock {
        pub fn should_render(
            &mut self,
            agent_id: &AgentID,
            agent_type: &AgentType,
            values: &AgentValues,
            attributes: &AgentAttributes,
            runtime: Runtime,
        ) {
            self.expect_render()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_type.clone()),
                    predicate::eq(values.clone()),
                    //predicate::eq(attributes.clone()),
                    predicate::eq(attributes.clone()),
                )
                .returning(move |_, _, _, _| Ok(runtime.clone()));
        }
    }

    fn testing_values(yaml_values: &str) -> AgentValues {
        serde_yaml::from_str(yaml_values).unwrap()
    }

    fn testing_agent_attributes(agent_id: &AgentID) -> AgentAttributes {
        AgentAttributes {
            agent_id: agent_id.to_string(),
        }
    }

    #[test]
    fn test_render() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(SIMPLE_AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args("--config_path=/some/path/config --foo=bar".into()),
            runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables
                .first()
                .unwrap()
                .args
                .clone()
                .get()
        );
    }

    #[test]
    fn test_render_with_empty_but_required_values() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        let values = AgentValues::default();
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let result = renderer.render(&agent_id, agent_type, values, attributes);
        assert_matches!(result.err().unwrap(), AgentTypeError::ValuesNotPopulated(vars) => {
            assert_eq!(vars, vec!["config_path".to_string()])
        })
    }

    #[test]
    fn test_render_with_missing_values() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(SIMPLE_AGENT_VALUES_REQUIRED_MISSING);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let result = renderer.render(&agent_id, agent_type, values, attributes);
        assert_matches!(result.err().unwrap(), AgentTypeError::ValuesNotPopulated(vars) => {
            assert_eq!(vars, vec!["config_path".to_string()])
        })
    }

    #[test]
    fn test_render_with_persister() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_FILES, &Environment::OnHost);
        let values = testing_values(AGENT_VALUES_WITH_FILES);
        let attributes = testing_agent_attributes(&agent_id);
        // The persister should receive filled variables with the path expanded.
        let path_as_string =
            format!("{SUPER_AGENT_DATA_DIR}/{GENERATED_FOLDER_NAME}/some-agent-id");
        let subagent_config_path = path_as_string.as_str();
        let filled_variables = agent_type
            .variables
            .clone()
            .fill_with_values(values.clone())
            .unwrap()
            .flatten();
        let expanded_path_filled_variables =
            TemplateRenderer::<MockConfigurationPersisterMock>::extend_variables_file_path(
                PathBuf::from(subagent_config_path),
                filled_variables.clone(),
            );

        let mut persister = MockConfigurationPersisterMock::new();
        persister.should_delete_agent_config(&agent_id, &expanded_path_filled_variables);
        persister.should_persist_agent_config(&agent_id, &expanded_path_filled_variables);

        let renderer = TemplateRenderer::default().with_config_persister(persister);

        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args(format!("--config1 {subagent_config_path}/config1.yml --config2 {subagent_config_path}/config2.d")),
            runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables
                .first()
                .unwrap()
                .args
                .clone()
                .get()
        );
    }

    #[test]
    fn test_render_with_persister_delete_error() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(SIMPLE_AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);
        let filled_variables = agent_type
            .variables
            .clone()
            .fill_with_values(values.clone())
            .unwrap()
            .flatten();

        let mut persister = MockConfigurationPersisterMock::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_not_delete_agent_config(&agent_id, &filled_variables, err);

        let renderer = TemplateRenderer::default().with_config_persister(persister);
        let expected_error = renderer
            .render(&agent_id, agent_type, values, attributes)
            .err()
            .unwrap();
        assert_matches!(
            expected_error,
            AgentTypeError::ConfigurationPersisterError(_)
        );
    }

    #[test]
    fn test_render_with_persister_persists_error() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(SIMPLE_AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);
        let filled_variables = agent_type
            .variables
            .clone()
            .fill_with_values(values.clone())
            .unwrap()
            .flatten();

        let mut persister = MockConfigurationPersisterMock::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_delete_agent_config(&agent_id, &filled_variables);
        persister.should_not_persist_agent_config(&agent_id, &filled_variables, err);

        let renderer = TemplateRenderer::default().with_config_persister(persister);

        let expected_error = renderer
            .render(&agent_id, agent_type, values, attributes)
            .err()
            .unwrap();
        assert_matches!(
            expected_error,
            AgentTypeError::ConfigurationPersisterError(_)
        );
    }

    #[test]
    fn test_render_agent_type_with_backoff_config() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF, &Environment::OnHost);
        let values = testing_values(BACKOFF_VALUES_YAML);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();

        let backoff_strategy = &runtime_config.deployment.on_host.unwrap().executables[0]
            .restart_policy
            .backoff_strategy;
        assert_eq!(
            BackoffStrategyType::Linear,
            backoff_strategy.backoff_type.clone().get()
        );
        assert_eq!(
            BackoffDelay::from_secs(10),
            backoff_strategy.backoff_delay.clone().get()
        );
        assert_eq!(
            MaxRetries::from(30),
            backoff_strategy.max_retries.clone().get()
        );
        assert_eq!(
            BackoffLastRetryInterval::from_secs(300),
            backoff_strategy.last_retry_interval.clone().get()
        );
    }

    #[test]
    fn test_render_agent_type_with_backoff_config_and_string_durations() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF, &Environment::OnHost);
        let values = testing_values(BACKOFF_VALUES_STRING_DURATION);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();

        let backoff_strategy = &runtime_config.deployment.on_host.unwrap().executables[0]
            .restart_policy
            .backoff_strategy;
        assert_eq!(
            BackoffStrategyType::Fixed,
            backoff_strategy.backoff_type.clone().get()
        );
        assert_eq!(
            BackoffDelay::from_secs((10 * 60) + 30),
            backoff_strategy.backoff_delay.clone().get()
        );
        assert_eq!(
            MaxRetries::from(30),
            backoff_strategy.max_retries.clone().get()
        );
        assert_eq!(
            BackoffLastRetryInterval::from_secs(300),
            backoff_strategy.last_retry_interval.clone().get()
        );
    }

    #[test]
    fn test_invalid_values_for_backoff_config() {
        // This is testing agent-type definition and values, but it is included here because it its related to
        // test_render_agent_type_with_backoff_config.
        let agent_type =
            AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF, &Environment::OnHost);

        let wrong_backoff_yamls = vec![
            WRONG_RETRIES_BACKOFF_CONFIG_YAML,
            WRONG_DELAY_BACKOFF_CONFIG_YAML,
            WRONG_INTERVAL_BACKOFF_CONFIG_YAML,
            WRONG_TYPE_BACKOFF_CONFIG_YAML,
        ];

        for yaml in wrong_backoff_yamls.into_iter() {
            let values = serde_yaml::from_str::<AgentValues>(yaml).unwrap();
            assert!(agent_type
                .variables
                .clone()
                .fill_with_values(values)
                .is_err())
        }
    }

    #[test]
    fn test_render_k8s_config_with_yaml_variables() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES, &Environment::K8s);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let expected_spec_yaml = r#"
values:
  key: value
  another_key:
    nested: nested_value
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
from_sub_agent: some-agent-id
collision_avoided: ${config.values}-${env:agent_id}
"#;
        let expected_spec_value: serde_yaml::Value =
            serde_yaml::from_str(expected_spec_yaml).unwrap();

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();

        let k8s = runtime_config.clone().deployment.k8s.unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    // Agent Type and Values definitions

    const SIMPLE_AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  config_path:
    description: "config file string"
    type: string
    required: true
  config_argument:
    description: "config argument"
    type: string
    required: false
    default: bar
deployment:
  on_host:
    executables:
      - path: /opt/first
        args: "--config_path=${nr-var:config_path} --foo=${nr-var:config_argument}"
        env: ""
"#;

    const SIMPLE_AGENT_VALUES: &str = r#"
config_path: /some/path/config
"#;

    const SIMPLE_AGENT_VALUES_REQUIRED_MISSING: &str = r#"
config_argument: value
"#;

    const AGENT_TYPE_WITH_FILES: &str = r#"
name: newrelic-infra
namespace: newrelic
version: 1.39.1
variables:
  config1:
    description: "One config file"
    type: file
    required: true
    file_path: "config1.yml"
  config2:
    description: "Set of config files"
    type: map[string]file
    required: true
    file_path: "config2.d"
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config1 ${nr-var:config1} --config2 ${nr-var:config2}"
        env: ""
"#;

    const AGENT_VALUES_WITH_FILES: &str = r#"
config1: |
  license_key: abc123
  staging: false
config2:
  file1.conf: |
    some content
  file2.conf: |
    some other content
"#;

    const AGENT_TYPE_WITH_BACKOFF: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
  backoff:
    delay:
      description: "Backoff delay"
      type: string
      required: false
      default: 1s
    retries:
      description: "Backoff retries"
      type: number
      required: false
      default: 3
    interval:
      description: "Backoff interval"
      type: string
      required: false
      default: 30s
    type:
      description: "Backoff strategy type"
      type: string
      required: true
deployment:
  on_host:
    executables:
      - path: /bin/otelcol
        args: "-c some-arg"
        env: ""
        restart_policy:
          backoff_strategy:
            type: ${nr-var:backoff.type}
            backoff_delay: ${nr-var:backoff.delay}
            max_retries: ${nr-var:backoff.retries}
            last_retry_interval: ${nr-var:backoff.interval}
"#;

    const BACKOFF_VALUES_YAML: &str = r#"
backoff:
  delay: 10s
  retries: 30
  interval: 300s
  type: linear
"#;

    const BACKOFF_VALUES_STRING_DURATION: &str = r#"
backoff:
  delay: 10m + 30s
  retries: 30
  interval: 5m
  type: fixed
"#;

    const WRONG_RETRIES_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: -30
  interval: 300
  type: linear
"#;

    const WRONG_DELAY_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: -10
  retries: 30
  interval: 300
  type: linear
"#;
    const WRONG_INTERVAL_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: 30
  interval: -300
  type: linear
"#;

    const WRONG_TYPE_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: 30
  interval: -300
  type: fafafa
"#;

    const K8S_AGENT_TYPE_YAML_VARIABLES: &str = r#"
name: k8s-agent-type
namespace: newrelic
version: 0.0.1
variables:
  config:
    values:
      description: "yaml values"
      type: yaml
      required: true
deployment:
  k8s:
    objects:
      cr1:
        apiVersion: group/version
        kind: ObjectKind
        metadata:
          name: test
        spec:
          values: ${nr-var:config.values}
          from_sub_agent: ${nr-sub:agent_id}
          collision_avoided: ${config.values}-${env:agent_id}
"#;

    const K8S_CONFIG_YAML_VALUES: &str = r#"
config:
  values:
    key: value
    another_key:
      nested: nested_value
      nested_list:
        - item1
        - item2
        - item3_nested: value
    empty_key:
"#;
}
