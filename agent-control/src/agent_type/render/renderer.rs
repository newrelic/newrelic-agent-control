use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{AGENT_CONTROL_DATA_DIR, GENERATED_FOLDER_NAME};
use crate::agent_type::render::persister::config_persister::ConfigurationPersister;
use crate::agent_type::{
    agent_attributes::AgentAttributes,
    definition::AgentType,
    error::AgentTypeError,
    runtime_config::Runtime,
    templates::Templateable,
    variable::{
        Variable,
        namespace::{Namespace, NamespacedVariableName},
    },
};
use crate::values::yaml_config::YAMLConfig;
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
        env_vars: HashMap<String, Variable>,
        secrets: HashMap<String, Variable>,
    ) -> Result<Runtime, AgentTypeError>;
}

#[derive(Debug, Default)]
pub struct TemplateRenderer<C: ConfigurationPersister> {
    persister: Option<C>,
    config_base_dir: Option<PathBuf>,
    sa_variables: HashMap<NamespacedVariableName, Variable>,
}

impl<C: ConfigurationPersister> Renderer for TemplateRenderer<C> {
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: YAMLConfig,
        attributes: AgentAttributes,
        env_vars: HashMap<String, Variable>,
        secrets: HashMap<String, Variable>,
    ) -> Result<Runtime, AgentTypeError> {
        // Get empty variables and runtime_config from the agent-type
        let (variables, runtime_config) = (agent_type.variables, agent_type.runtime_config);

        // Values are expanded substituting all ${nr-env...} with environment variables.
        // Notice that only environment variables and secrets are taken into consideration (no other vars for example)
        let values_expanded = values.template_with(&secrets)?;

        // Fill agent variables
        // `filled_variables` needs to be mutable, in case there are `File` or `MapStringFile` variables, whose path
        // needs to be expanded, checkout out the TODO below for details.
        let mut filled_variables = variables.fill_with_values(values_expanded)?.flatten();

        Self::check_all_vars_are_populated(&filled_variables)?;

        // TODO: the persister performs specific actions for file and `File` and `MapStringFile` variables kind only.
        // If another kind with specific actions is introduced, the kind definition should be refactored to allow
        // performing additional actions when filling variables with values.
        if let Some(persister) = &self.persister {
            let sub_agent_config_path = self
                .config_base_dir
                .as_ref()
                .unwrap_or(&PathBuf::from(AGENT_CONTROL_DATA_DIR))
                .join(GENERATED_FOLDER_NAME)
                .join(agent_id);
            filled_variables =
                Self::extend_variables_file_path(sub_agent_config_path, filled_variables);
            persister.delete_agent_config(agent_id)?;
            persister.persist_agent_config(agent_id, &filled_variables)?;
        }

        // Setup namespaced variables
        let ns_variables = self.build_namespaced_variables(filled_variables, env_vars, &attributes);
        // Render runtime config
        let rendered_runtime_config = runtime_config.template_with(&ns_variables)?;

        Ok(rendered_runtime_config)
    }
}

impl<C: ConfigurationPersister> TemplateRenderer<C> {
    /// Adds variables to the renderer with the agent-control namespace.
    pub fn with_agent_control_variables(
        self,
        variables: impl Iterator<Item = (String, Variable)>,
    ) -> Self {
        Self {
            sa_variables: variables
                .map(|(name, value)| {
                    (
                        Namespace::AgentControl.namespaced_name(name.as_str()),
                        value,
                    )
                })
                .collect(),
            ..self
        }
    }

    pub fn with_config_persister(self, c: C, config_base_dir: PathBuf) -> Self {
        Self {
            persister: Some(c),
            config_base_dir: Some(config_base_dir),
            ..self
        }
    }

    // Extends the path of all variables with the sub-agent generated config path.
    fn extend_variables_file_path(
        config_path: PathBuf,
        mut variables: HashMap<String, Variable>,
    ) -> HashMap<String, Variable> {
        for var in variables.values_mut() {
            var.extend_file_path(config_path.as_path());
        }
        variables
    }

    fn check_all_vars_are_populated(
        variables: &HashMap<String, Variable>,
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
        variables: HashMap<String, Variable>,
        env_vars: HashMap<String, Variable>,
        attributes: &AgentAttributes,
    ) -> HashMap<NamespacedVariableName, Variable> {
        // Set the namespaced name to variables
        let vars_iter = variables
            .into_iter()
            .map(|(name, var)| (Namespace::Variable.namespaced_name(&name), var));
        // Get the namespaced variables from sub-agent attributes
        let sub_agent_vars_iter = attributes.sub_agent_variables().into_iter();

        // Join all variables together
        vars_iter
            .chain(sub_agent_vars_iter)
            .chain(env_vars)
            .chain(self.sa_variables.clone())
            .collect::<HashMap<NamespacedVariableName, Variable>>()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        agent_control::run::Environment,
        agent_type::{
            definition::AgentType,
            render::persister::{
                config_persister::{PersistError, tests::MockConfigurationPersister},
                config_persister_file::ConfigurationPersisterFile,
            },
            runtime_config::{
                onhost::Args,
                restart_policy::{
                    BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
                },
            },
        },
    };
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagementError;
    use mockall::{mock, predicate};
    use tempfile::TempDir;

    mock! {
         pub(crate) Renderer {}

         impl Renderer for Renderer {
             fn render(
                &self,
                agent_id: &AgentID,
                agent_type: AgentType,
                values: YAMLConfig,
                attributes: AgentAttributes,
                env_vars: HashMap<String, Variable>,
                secrets: HashMap<String, Variable>,
            ) -> Result<Runtime, AgentTypeError>;
         }
    }

    impl MockRenderer {
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
                    predicate::always(), // Not caring for env vars
                    predicate::always(), // Not caring for secrets
                )
                .returning(move |_, _, _, _, _, _| Ok(runtime.clone()));
        }
    }

    fn testing_values(yaml_values: &str) -> YAMLConfig {
        serde_yaml::from_str(yaml_values).unwrap()
    }

    pub fn testing_agent_attributes(agent_id: &AgentID) -> AgentAttributes {
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
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
        let result = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );
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
        let result = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );
        assert_matches!(result.unwrap_err(), AgentTypeError::ValuesNotPopulated(vars) => {
            assert_eq!(vars, vec!["config_path".to_string()])
        })
    }

    #[test]
    fn test_render_with_persister() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_FILES, &Environment::OnHost);
        let values = AGENT_VALUES_WITH_FILES;
        let attributes = testing_agent_attributes(&agent_id);
        // The persister should receive filled variables with the path expanded.
        let data_dir: TempDir = TempDir::new().unwrap();
        let path_as_string = data_dir.path().join(GENERATED_FOLDER_NAME).join(&agent_id);
        let filled_variables = agent_type.fill_variables(values);

        let expanded_path_filled_variables =
            TemplateRenderer::<MockConfigurationPersister>::extend_variables_file_path(
                path_as_string.clone(),
                filled_variables.clone(),
            );

        let mut persister = MockConfigurationPersister::new();
        persister.should_delete_agent_config(&agent_id);
        persister.should_persist_agent_config(&agent_id, &expanded_path_filled_variables);

        let renderer = TemplateRenderer::default()
            .with_config_persister(persister, data_dir.path().to_path_buf());

        let runtime_config = renderer
            .render(
                &agent_id,
                agent_type,
                testing_values(values),
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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

        let mut persister = MockConfigurationPersister::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_not_delete_agent_config(&agent_id, err);

        let data_dir: TempDir = TempDir::new().unwrap();
        let renderer = TemplateRenderer::default()
            .with_config_persister(persister, data_dir.path().to_path_buf());
        let expected_error = renderer
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
        let values = SIMPLE_AGENT_VALUES;
        let attributes = testing_agent_attributes(&agent_id);
        let filled_variables = agent_type.fill_variables(values);

        let mut persister = MockConfigurationPersister::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_delete_agent_config(&agent_id);
        persister.should_not_persist_agent_config(&agent_id, &filled_variables, err);

        let data_dir: TempDir = TempDir::new().unwrap();
        let renderer = TemplateRenderer::default()
            .with_config_persister(persister, data_dir.path().to_path_buf());

        let expected_error = renderer
            .render(
                &agent_id,
                agent_type,
                testing_values(values),
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
            assert!(
                agent_type
                    .variables
                    .clone()
                    .fill_with_values(values)
                    .is_err()
            )
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
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let k8s = runtime_config.deployment.k8s.unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    fn test_render_with_env_variables() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let env_vars = HashMap::from([
            (
                Namespace::EnvironmentVariable.namespaced_name("MY_VARIABLE"),
                Variable::new_final_string_variable("my-value".to_string()),
            ),
            (
                Namespace::EnvironmentVariable.namespaced_name("MY_VARIABLE_2"),
                Variable::new_final_string_variable("my-value-2".to_string()),
            ),
        ]);

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
        let runtime_config = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            env_vars,
            HashMap::new(),
        );

        let k8s = runtime_config.unwrap().deployment.k8s.unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
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

        let secrets = HashMap::from([
            (
                Namespace::EnvironmentVariable.namespaced_name("DOUBLE_EXPANSION"),
                Variable::new_final_string_variable("test".to_string()),
            ),
            (
                Namespace::EnvironmentVariable.namespaced_name("DOUBLE_EXPANSION_2"),
                Variable::new_final_string_variable("test-2".to_string()),
            ),
        ]);

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
        let runtime_config = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            HashMap::new(),
            secrets,
        );

        let k8s = runtime_config.unwrap().deployment.k8s.unwrap();
        let values = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();
        assert_eq!(expected_spec_value, values.clone());
    }

    #[test]
    fn test_render_with_env_variables_not_found() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );

        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }

    #[test]
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
          namespace: test-namespace
        substituted: ${nr-env:MY_VARIABLE}
"#,
            &Environment::K8s,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let env_vars = HashMap::from([(
            Namespace::EnvironmentVariable.namespaced_name("my_variable"),
            Variable::new_final_string_variable("my-value".to_string()),
        )]);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer.render(
            &agent_id,
            agent_type,
            values,
            attributes,
            env_vars,
            HashMap::new(),
        );

        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }

    #[test]
    fn test_render_expand_agent_control_variables() {
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
      args: "${nr-ac:sa-fake-var}"
"#,
            &Environment::OnHost,
        );
        let values = testing_values("");
        let attributes = testing_agent_attributes(&agent_id);

        let agent_control_variables = HashMap::from([(
            "sa-fake-var".to_string(),
            Variable::new_final_string_variable("fake_value".to_string()),
        )]);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default()
            .with_agent_control_variables(agent_control_variables.into_iter());
        let runtime_config = renderer
            .render(
                &agent_id,
                agent_type,
                values,
                attributes,
                HashMap::new(),
                HashMap::new(),
            )
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
          namespace: test-namespace
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
          namespace: test-namespace
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
