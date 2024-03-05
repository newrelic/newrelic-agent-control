use crate::{
    sub_agent::persister::config_persister::ConfigurationPersister, super_agent::config::AgentID,
};

use super::{
    agent_attributes::AgentAttributes, agent_values::AgentValues, definition::AgentType,
    error::AgentTypeError, runtime_config::Runtime,
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
    persister: Option<C>, // TODO: check if it should be optional or we should have different Renderer implementations.
                          // depending on what fields are supported for each environment.
}

impl<C: ConfigurationPersister> Renderer for TemplateRenderer<C> {
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: AgentValues,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError> {
        // TODO: `agent_type.template` logic (and underlying helper methods) should be moved here.
        let populated_agent = agent_type.template(values, attributes)?;
        if let Some(persister) = &self.persister {
            persister.delete_agent_config(agent_id, &populated_agent)?;
            persister.persist_agent_config(agent_id, &populated_agent)?;
        }
        Ok(populated_agent.runtime_config)
    }
}

impl<C: ConfigurationPersister> Default for TemplateRenderer<C> {
    fn default() -> Self {
        Self { persister: None }
    }
}

impl<C: ConfigurationPersister> TemplateRenderer<C> {
    pub fn with_config_persister(self, c: C) -> Self {
        Self { persister: Some(c) }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    use fs::directory_manager::DirectoryManagementError;

    use crate::{
        agent_type::{definition::AgentType, environment::Environment, runtime_config::Args},
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

    const AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  config_path:
    description: "config file string"
    type: string
    required: true
deployment:
  on_host:
    executables:
      - path: /opt/first
        args: "--config_path=${nr-var:config_path}"
        env: ""
"#;

    const AGENT_VALUES: &str = r#"
config_path: /some/path/config
"#;
    const ABS_PATH: &str = "/tmp";

    fn testing_values(yaml_values: &str) -> AgentValues {
        serde_yaml::from_str(yaml_values).unwrap()
    }

    fn testing_agent_attributes(agent_id: &AgentID) -> AgentAttributes {
        AgentAttributes {
            generated_configs_path: PathBuf::from(ABS_PATH),
            agent_id: agent_id.to_string(),
        }
    }

    #[test]
    fn test_render_with_no_persister() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        //let agent_type = testing_agent_type(AGENT_TYPE);
        let agent_type = AgentType::build_for_testing(AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let renderer: TemplateRenderer<ConfigurationPersisterFile> = TemplateRenderer::default();
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args("--config_path=/some/path/config".into()),
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
    fn test_render_with_persister() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        // TODO: populated_needed is needed to test the current configuration persister arguments, it should not be needed here.
        let populated_agent = agent_type
            .clone()
            .template(values.clone(), attributes.clone())
            .unwrap();

        let mut persister = MockConfigurationPersisterMock::new();
        persister.should_delete_agent_config(&agent_id, &populated_agent.get_variables());
        persister.should_persist_agent_config(&agent_id, &populated_agent.get_variables());

        let renderer = TemplateRenderer {
            persister: Some(persister),
        };
        let runtime_config = renderer
            .render(&agent_id, agent_type, values, attributes)
            .unwrap();
        assert_eq!(
            Args("--config_path=/some/path/config".into()),
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
        let agent_type = AgentType::build_for_testing(AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        // TODO: populated_needed is needed here to test the current configuration persister arguments, it should not be needed here.
        let populated_agent = agent_type
            .clone()
            .template(values.clone(), attributes.clone())
            .unwrap();

        let mut persister = MockConfigurationPersisterMock::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_not_delete_agent_config(&agent_id, &populated_agent.get_variables(), err);

        let renderer = TemplateRenderer {
            persister: Some(persister),
        };
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
        let agent_type = AgentType::build_for_testing(AGENT_TYPE, &Environment::OnHost);
        let values = testing_values(AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        // TODO: populated_needed is needed here to test the current configuration persister arguments, it should not be needed here.
        let populated_agent = agent_type
            .clone()
            .template(values.clone(), attributes.clone())
            .unwrap();

        let mut persister = MockConfigurationPersisterMock::new();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        persister.should_delete_agent_config(&agent_id, &populated_agent.get_variables());
        persister.should_not_persist_agent_config(&agent_id, &populated_agent.get_variables(), err);

        let renderer = TemplateRenderer {
            persister: Some(persister),
        };

        let expected_error = renderer
            .render(&agent_id, agent_type, values, attributes)
            .err()
            .unwrap();
        assert_matches!(
            expected_error,
            AgentTypeError::ConfigurationPersisterError(_)
        );
    }
}
