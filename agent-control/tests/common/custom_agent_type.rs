use fs::file::LocalFile;
use fs::file::writer::FileWriter;
use newrelic_agent_control::agent_control::defaults::DYNAMIC_AGENT_TYPES_DIR;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::definition::AgentTypeDefinition;
use std::path::Path;

pub struct CommonCustomAgentTypeBuilder {
    pub agent_type_id: AgentTypeID,
    pub variables: Option<serde_json::Value>,
}

impl CommonCustomAgentTypeBuilder {
    pub fn new(agent_type_id: AgentTypeID) -> Self {
        Self {
            agent_type_id,
            variables: None,
        }
    }

    pub fn with_variables(mut self, variables: &str) -> Self {
        self.variables = Some(serde_saphyr::from_str(variables).unwrap());
        self
    }

    pub fn write(&self, local_dir: &Path, content: &str) -> String {
        // The id (`namespace/name:version`) has `/` and `:`, which are not portable in file names.
        let file_stem = self.agent_type_id.to_string().replace(['/', ':'], "_");
        let agent_type_file_path = local_dir
            .join(DYNAMIC_AGENT_TYPES_DIR)
            .join(format!("{file_stem}.yaml"));

        let parsed_agent_type = AgentTypeDefinition::from_slice(content.as_bytes());
        assert!(
            parsed_agent_type.is_ok(),
            "CustomAgentType did not produce valid AgentTypeDefinition: {}\n{content}",
            parsed_agent_type.err().unwrap(),
        );

        std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
        LocalFile
            .write(&agent_type_file_path, content.to_string())
            .expect("failed to write custom agent type");
        self.agent_type_id.to_string()
    }
}
