//! This modules defines the Agent Type variables, including it serialized representation and the corresponding
//! functionality.
//!
//! Most types names follow this convention: the suffix `Definition` means that the type is used to represent the
//! static data that can be deserialized from the information in the Agent Type registry. Eg: [VariableDefinition].
//! On the other hand, the type without the `Definition` suffix represents the same information but also includes
//! some runtime information. Eg: [Variable].

pub mod constraints;
pub mod fields;
pub mod namespace;
pub mod tree;
pub mod variable_type;
pub mod variants;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    trivial_value::TrivialValue,
    variable::{
        constraints::VariableConstraints, fields::StringFields,
        variable_type::VariableTypeDefinition,
    },
};

use fields::Fields;
use variable_type::VariableType;

/// Static Variable definition defines the supported fields for a variable in an Agent Type.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct VariableDefinition {
    pub(crate) description: String,
    #[serde(flatten)]
    variable_type: VariableTypeDefinition,
}

/// [VariableDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub struct Variable {
    pub(crate) description: String,
    variable_type: VariableType,
}

impl VariableDefinition {
    /// Returns the corresponding [Variable] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> Variable {
        Variable {
            description: self.description,
            variable_type: self.variable_type.with_config(constraints),
        }
    }
}

impl Variable {
    pub fn new_final_string_variable(final_value: String) -> Self {
        Self {
            description: String::new(),
            variable_type: VariableType::String(StringFields {
                inner: Fields {
                    required: false,
                    default: None,
                    final_value: Some(final_value),
                },
                variants: Default::default(),
            }),
        }
    }

    pub fn is_required(&self) -> bool {
        self.variable_type.is_required()
    }

    pub fn get_final_value(&self) -> Option<TrivialValue> {
        self.variable_type.get_final_value()
    }

    pub fn get_file_path(&self) -> Option<&PathBuf> {
        self.variable_type.get_file_path()
    }

    pub fn extend_file_path(&mut self, path: &Path) {
        if let Some(current_path) = self.get_file_path() {
            self.variable_type.set_file_path(path.join(current_path))
        }
    }

    pub fn merge_with_yaml_value(&mut self, yaml: serde_yaml::Value) -> Result<(), AgentTypeError> {
        self.variable_type.merge_with_yaml_value(yaml)
    }

    /// Returns the replacement value that will be used to substitute
    /// the placeholder from an agent_type when templating a config
    pub fn get_template_value(&self) -> Option<TrivialValue> {
        match self.get_file_path() {
            // For MapStringFile and file the file_path includes the full path with agent_configs_path
            Some(p) => Some(TrivialValue::String(p.to_string_lossy().into())),
            _ => self.get_final_value(),
        }
    }

    pub fn kind(&self) -> &VariableType {
        &self.variable_type
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use crate::agent_type::variable::{
        VariableDefinition,
        fields::{Fields, FieldsDefinition, FieldsWithPath, StringFields, StringFieldsDefinition},
        tree::Tree,
        variable_type::{VariableType, VariableTypeDefinition},
        variants::VariantsConfig,
    };

    use super::Variable;

    impl From<Fields<serde_yaml::Value>> for Variable {
        fn from(kind_value: Fields<serde_yaml::Value>) -> Self {
            Self {
                description: String::new(),
                variable_type: VariableType::Yaml(kind_value),
            }
        }
    }

    impl Variable {
        pub(crate) fn new<T>(
            description: String,
            required: bool,
            default: Option<T>,
            final_value: Option<T>,
        ) -> Self
        where
            T: PartialEq,
            VariableType: From<Fields<T>>,
        {
            Self {
                description,
                variable_type: Fields::new(required, default, final_value).into(),
            }
        }

        pub(crate) fn new_string(
            description: String,
            required: bool,
            default: Option<String>,
            final_value: Option<String>,
        ) -> Self {
            Self {
                description,
                variable_type: StringFields::new(
                    required,
                    default,
                    Default::default(),
                    final_value,
                )
                .into(),
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
            VariableType: From<FieldsWithPath<T>>,
        {
            Self {
                description,
                variable_type: FieldsWithPath::new(required, default, final_value, file_path)
                    .into(),
            }
        }
    }

    #[test]
    fn variable_definition_tree_deserialize() {
        let value = r#"
foo:
  bar:
    var_name:
      description: "some description"
      type: string
      required: false
      default: "a"
      variants:
        ac_config_field: "foo.bar.var_name"
        values: ["a", "b"]
"#;
        let tree: Tree<VariableDefinition> = serde_yaml::from_str(value).unwrap();
        let expected: Tree<VariableDefinition> = Tree::Mapping(HashMap::from([(
            "foo".to_string(),
            Tree::Mapping(HashMap::from([(
                "bar".to_string(),
                Tree::Mapping(HashMap::from([(
                    "var_name".to_string(),
                    Tree::End(VariableDefinition {
                        description: "some description".to_string(),
                        variable_type: VariableTypeDefinition::String(StringFieldsDefinition {
                            inner: FieldsDefinition {
                                required: false,
                                default: Some("a".to_string()),
                            },
                            variants: VariantsConfig {
                                ac_config_field: Some("foo.bar.var_name".to_string()),
                                values: vec!["a".to_string(), "b".to_string()].into(),
                            },
                        }),
                    }),
                )])),
            )])),
        )]));
        assert_eq!(tree, expected);
    }
}
