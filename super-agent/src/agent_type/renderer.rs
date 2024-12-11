use super::{
    agent_attributes::AgentAttributes,
    definition::AgentType,
    error::AgentTypeError,
    runtime_config::Runtime,
    runtime_config_templates::Templateable,
    variable::{
        definition::VariableDefinition,
        namespace::{Namespace, NamespacedVariableName},
    },
};
use crate::agent_type::environment_variable::retrieve_env_var_variables;
use crate::values::yaml_config::YAMLConfig;
use crate::{
    sub_agent::persister::config_persister::ConfigurationPersister,
    super_agent::{config::AgentID, defaults::GENERATED_FOLDER_NAME},
};
use std::{collections::HashMap, path::PathBuf};

/// Defines how to render an AgentType and obtain the runtime configuration needed to execute a sub agent.
pub trait Renderer {
    /// Renders the runtime configuration in an [AgentType] using the provided values and attributes.
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: YAMLConfig,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError>;
}

pub struct TemplateRenderer<C: ConfigurationPersister> {
    persister: Option<C>,
    config_base_dir: PathBuf,
    sa_variables: HashMap<NamespacedVariableName, VariableDefinition>,
}

impl<C: ConfigurationPersister> Renderer for TemplateRenderer<C> {
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: YAMLConfig,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError> {
        // Values are expanded substituting all ${nr-env...} with environment variables.
        // Notice that only environment variables are taken into consideration (no other vars for example)
        let environment_variables = retrieve_env_var_variables();
        let values_expanded = values.template_with(&environment_variables)?;

        // Fill agent variables
        // `filled_variables` needs to be mutable, in case there are `File` or `MapStringFile` variables, whose path
        // needs to be expanded, checkout out the TODO below for details.
        let mut filled_variables = agent_type
            .variables
            .fill_with_values(values_expanded)?
            .flatten();

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
        let ns_variables =
            self.build_namespaced_variables(filled_variables, environment_variables, &attributes);
        // Render runtime config
        let rendered_runtime_config = agent_type.runtime_config.template_with(&ns_variables)?;

        Ok(rendered_runtime_config)
    }
}

impl<C: ConfigurationPersister> TemplateRenderer<C> {
    pub fn new(config_base_dir: PathBuf) -> Self {
        Self {
            persister: None,
            config_base_dir,
            sa_variables: HashMap::new(),
        }
    }

    /// Adds variables to the renderer with the super-agent namespace.
    pub fn with_super_agent_variables(
        self,
        variables: impl Iterator<Item = (String, VariableDefinition)>,
    ) -> Self {
        Self {
            sa_variables: variables
                .map(|(name, value)| (Namespace::SuperAgent.namespaced_name(name.as_str()), value))
                .collect(),
            ..self
        }
    }

    pub fn with_config_persister(self, c: C) -> Self {
        Self {
            persister: Some(c),
            ..self
        }
    }

    // Returns the config path for a sub-agent.
    fn subagent_config_path(&self, agent_id: &AgentID) -> PathBuf {
        self.config_base_dir
            .join(GENERATED_FOLDER_NAME)
            .join(agent_id)
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
        &self,
        variables: HashMap<String, VariableDefinition>,
        environment_variables: HashMap<String, VariableDefinition>,
        attributes: &AgentAttributes,
    ) -> HashMap<NamespacedVariableName, VariableDefinition> {
        // Set the namespaced name to variables
        let vars_iter = variables
            .into_iter()
            .map(|(name, var)| (Namespace::Variable.namespaced_name(&name), var));
        // Get the namespaced variables from sub-agent attributes
        let sub_agent_vars_iter = attributes.sub_agent_variables().into_iter();

        // Join all variables together
        vars_iter
            .chain(sub_agent_vars_iter)
            .chain(environment_variables)
            .chain(self.sa_variables.clone())
            .collect::<HashMap<NamespacedVariableName, VariableDefinition>>()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
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
            config_persister::{tests::MockConfigurationPersisterMock, PersistError},
            config_persister_file::ConfigurationPersisterFile,
        },
    };
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagementError;
    use mockall::{mock, predicate};
    use serial_test::serial;
    use std::env;

    fn test_data_dir() -> PathBuf {
        PathBuf::from("/some/path")
    }

    impl<C: ConfigurationPersister> Default for TemplateRenderer<C> {
        fn default() -> Self {
            Self {
                persister: None,
                // TODO replace this
                config_base_dir: test_data_dir(),
                sa_variables: HashMap::new(),
            }
        }
    }

    mock! {
         pub(crate) RendererMock {}

         impl Renderer for RendererMock {
             fn render(
                &self,
                agent_id: &AgentID,
                agent_type: AgentType,
                values: YAMLConfig,
                attributes: AgentAttributes,
            ) -> Result<Runtime, AgentTypeError>;
         }
    }

    impl MockRendererMock {
        pub fn should_render(
            &mut self,
            agent_id: &AgentID,
            agent_type: &AgentType,
            values: &YAMLConfig,
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

    fn testing_values(yaml_values: &str) -> YAMLConfig {
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
                .executable
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
        let values = YAMLConfig::default();
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let result = renderer.render(&agent_id, agent_type, values, attributes);
        assert_matches!(result.unwrap_err(), AgentTypeError::ValuesNotPopulated(vars) => {
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
        assert_matches!(result.unwrap_err(), AgentTypeError::ValuesNotPopulated(vars) => {
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
        let path_as_string = test_data_dir().join(GENERATED_FOLDER_NAME).join(&agent_id);
        let filled_variables = agent_type
            .variables
            .clone()
            .fill_with_values(values.clone())
            .unwrap()
            .flatten();
        let expanded_path_filled_variables =
            TemplateRenderer::<MockConfigurationPersisterMock>::extend_variables_file_path(
                path_as_string.clone(),
                filled_variables.clone(),
            );

        let mut persister = MockConfigurationPersisterMock::new();
        persister.should_delete_agent_config(&agent_id);
        persister.should_persist_agent_config(&agent_id, &expanded_path_filled_variables);

        let renderer = TemplateRenderer::default().with_config_persister(persister);

        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args(format!(
                "--config1 {}/config1.yml --config2 {}/config2.d",
                &path_as_string.to_string_lossy(),
                &path_as_string.to_string_lossy()
            )),
            runtime_config
                .deployment
                .on_host
                .unwrap()
                .executable
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

        let mut persister = MockConfigurationPersisterMock::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_not_delete_agent_config(&agent_id, err);

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
        persister.should_delete_agent_config(&agent_id);
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

        let backoff_strategy = &runtime_config
            .deployment
            .on_host
            .unwrap()
            .executable
            .unwrap()
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

        let backoff_strategy = &runtime_config
            .deployment
            .on_host
            .unwrap()
            .executable
            .unwrap()
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
            let values = serde_yaml::from_str::<YAMLConfig>(yaml).unwrap();
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
  another_key:
    nested: nested_value ${UNTOUCHED}
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
from_sub_agent: some-agent-id
text_values: "key: value\nkey2: ${UNTOUCHED}\n\n"
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;
        let expected_spec_value: serde_yaml::Value =
            serde_yaml::from_str(expected_spec_yaml).unwrap();

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();

        let k8s = runtime_config.deployment.k8s.unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    #[serial]
    fn test_render_with_env_variables() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        env::set_var("MY_VARIABLE", "my-value");
        env::set_var("MY_VARIABLE_2", "my-value-2");

        let expected_spec_yaml = r#"
values:
  another_key:
    nested: nested_value ${UNTOUCHED}
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
from_sub_agent: some-agent-id
substituted: my-value
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
substituted_2: my-value-2
"#;

        let expected_spec_value: serde_yaml::Value =
            serde_yaml::from_str(expected_spec_yaml).unwrap();

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(&agent_id, agent_type, values, attributes);

        env::remove_var("MY_VARIABLE");
        env::remove_var("MY_VARIABLE_2");

        let k8s = runtime_config.unwrap().deployment.k8s.unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    #[serial]
    fn test_render_double_expansion_with_env_variables() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES, &Environment::K8s);
        let values = testing_values(
            r#"
config:
  text_values:
    key: value
    key2: ${UNTOUCHED}
  values:
    key: ${nr-env:DOUBLE_EXPANSION}
    key-2: ${nr-env:DOUBLE_EXPANSION_2}
"#,
        );
        let attributes = testing_agent_attributes(&agent_id);

        env::set_var("DOUBLE_EXPANSION", "test");
        env::set_var("DOUBLE_EXPANSION_2", "test-2");

        let expected_spec_yaml = r#"
values:
  key: test
  key-2: test-2
from_sub_agent: some-agent-id
text_values: "key: value\nkey2: ${UNTOUCHED}\n\n"
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;

        let expected_spec_value: serde_yaml::Value =
            serde_yaml::from_str(expected_spec_yaml).unwrap();

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(&agent_id, agent_type, values, attributes);

        env::remove_var("DOUBLE_EXPANSION");
        env::remove_var("DOUBLE_EXPANSION_2");

        let k8s = runtime_config.unwrap().deployment.k8s.unwrap();
        let values = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();
        assert_eq!(expected_spec_value, values.clone());
    }

    #[test]
    #[serial]
    fn test_render_with_env_variables_not_found() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(&agent_id, agent_type, values, attributes);

        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }

    #[test]
    #[serial]
    fn test_render_with_env_variables_are_case_sensitive() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            r#"
name: k8s-agent-type
namespace: newrelic
version: 0.0.1
variables:
  k8s:
    config:
      values:
        description: "yaml values"
        type: yaml
        required: true
      text_values:
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
        substituted: ${nr-env:MY_VARIABLE}
"#,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);
        env::set_var("my_variable", "my-value");

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(&agent_id, agent_type, values, attributes);

        env::remove_var("my_variable");
        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }
    #[test]
    fn test_render_expand_super_agent_variables() {
        let agent_id = AgentID::new("some-agent-id").unwrap();

        let agent_type = AgentType::build_for_testing(
            r#"
namespace: newrelic
name: first
version: 0.1.0
variables: {}
deployment:
  on_host:
    executable:
      path: /opt/first
      args: "${nr-sa:sa-fake-var}"
"#,
            &Environment::OnHost,
        );
        let values = testing_values("");
        let attributes = testing_agent_attributes(&agent_id);

        let super_agent_variables = HashMap::from([(
            "sa-fake-var".to_string(),
            VariableDefinition::new_final_string_variable("fake_value".to_string()),
        )]);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default()
            .with_super_agent_variables(super_agent_variables.into_iter());
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args("fake_value".into()),
            runtime_config
                .deployment
                .on_host
                .unwrap()
                .executable
                .unwrap()
                .args
                .clone()
                .get()
        );
    }

    // Agent Type and Values definitions

    const SIMPLE_AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  common:
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
    executable:
      path: /opt/first
      args: "--config_path=${nr-var:config_path} --foo=${nr-var:config_argument}"
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
  common:
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
    executable:
      path: /usr/bin/newrelic-infra
      args: "--config1 ${nr-var:config1} --config2 ${nr-var:config2}"
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
  on_host:
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
    executable:
      path: /bin/otelcol
      args: "-c some-arg"
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
  k8s:
    config:
      values:
        description: "yaml values"
        type: yaml
        required: true
      text_values:
        description: "text values"
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
          text_values: |
            ${nr-var:config.text_values}
          collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;

    const K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES: &str = r#"
name: k8s-agent-type
namespace: newrelic
version: 0.0.1
variables:
  k8s:
    config:
      values:
        description: "yaml values"
        type: yaml
        required: true
      text_values:
        description: "text values"
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
          substituted: ${nr-env:MY_VARIABLE}
          collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
          substituted_2: ${nr-env:MY_VARIABLE_2}
"#;

    const K8S_CONFIG_YAML_VALUES: &str = r#"
config:
  text_values:
    key: value
    key2: ${UNTOUCHED}
  values:
    another_key:
      nested: nested_value ${UNTOUCHED}
      nested_list:
        - item1
        - item2
        - item3_nested: value
    empty_key:"#;
}
