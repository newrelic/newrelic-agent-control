use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::agent_type::{
    agent_types::VariableType, error::AgentTypeError, trivial_value::TrivialValue,
};

use super::kind::Kind;

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum Spec {
    SpecEnd(EndSpec),
    SpecMapping(HashMap<String, Spec>),
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct EndSpec {
    pub(crate) description: String,
    #[serde(flatten)]
    kind: Kind,
}

impl EndSpec {
    pub fn variable_type(&self) -> VariableType {
        self.kind.variable_type()
    }

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

    pub fn is_not_required_without_default(&self) -> bool {
        self.kind.is_not_required_without_default()
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::config::agent_type::variable_spec::{
        kind::Kind,
        kind_value::{KindValue, KindValueWithPath},
    };

    use super::EndSpec;

    #[allow(private_bounds)] // Not sure how to solve this, so, for the moment...
    impl EndSpec {
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
