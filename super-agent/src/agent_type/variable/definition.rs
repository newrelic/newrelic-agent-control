use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_type::{error::AgentTypeError, trivial_value::TrivialValue};

use super::kind::Kind;

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum VariableDefinitionTree {
    End(VariableDefinition),
    Mapping(HashMap<String, VariableDefinitionTree>),
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct VariableDefinition {
    pub(crate) description: String,
    #[serde(flatten)]
    kind: Kind,
}

impl VariableDefinition {
    pub fn is_required(&self) -> bool {
        self.kind.is_required()
    }

    pub fn get_final_value(&self) -> Option<TrivialValue> {
        self.kind.get_final_value()
    }

    pub fn get_file_path(&self) -> Option<&PathBuf> {
        self.kind.get_file_path()
    }

    pub fn set_file_path(&mut self, path: PathBuf) {
        self.kind.set_file_path(path)
    }

    pub fn merge_with_yaml_value(&mut self, yaml: serde_yaml::Value) -> Result<(), AgentTypeError> {
        self.kind.merge_with_yaml_value(yaml)
    }

    /// get_template_value returns the replacement value that will be used to substitute
    /// the placeholder from an agent_type when templating a config
    pub fn get_template_value(&self) -> Option<TrivialValue> {
        match self.get_file_path() {
            // For MapStringFile and file the file_path includes the full path with agent_configs_path
            Some(p) => Some(TrivialValue::String(p.to_string_lossy().into())),
            _ => self.get_final_value(),
        }
    }

    pub fn kind(&self) -> &Kind {
        &self.kind
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::agent_type::variable::{
        kind::Kind,
        kind_value::{KindValue, KindValueWithPath},
    };

    use super::VariableDefinition;

    impl VariableDefinition {
        pub(crate) fn new<T>(
            description: String,
            required: bool,
            default: Option<T>,
            final_value: Option<T>,
        ) -> Self
        where
            T: PartialEq,
            Kind: From<KindValue<T>>,
        {
            Self {
                description,
                kind: KindValue::new(required, default, final_value).into(),
            }
        }

        pub(crate) fn new_with_file_path<T>(
            description: String,
            required: bool,
            default: Option<T>,
            final_value: Option<T>,
            file_path: PathBuf,
        ) -> Self
        where
            T: PartialEq,
            Kind: From<KindValueWithPath<T>>,
        {
            Self {
                description,
                kind: KindValueWithPath::new(required, default, final_value, file_path).into(),
            }
        }
    }
}
