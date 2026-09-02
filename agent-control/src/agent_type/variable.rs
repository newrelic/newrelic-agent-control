//! This modules defines the Agent Type variables, including it serialized representation and the corresponding
//! functionality.
//!
//! Most types names follow this convention: the suffix `Definition` means that the type is used to represent the
//! static data that can be deserialized from the information in the Agent Type registry. Eg: [VariableDefinition].
//! On the other hand, the type without the `Definition` suffix represents the same information but also includes
//! some runtime information. Eg: [Variable].

pub mod constraints;
pub mod dynamic_variables;
pub mod fields;
pub mod name;
pub mod namespace;
pub mod tree;
pub mod variable_type;
pub mod variants;

use crate::agent_type::{
    error::AgentTypeError,
    variable::{
        constraints::VariableConstraints, fields::StringFields,
        variable_type::VariableTypeDefinition,
    },
    variable_value::VariableValue,
};
use fields::Fields;
use serde::{Deserialize, Serialize};
use variable_type::VariableType;

/// Static Variable definition defines the supported fields for a variable in an Agent Type.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct VariableDefinition {
    #[serde(flatten)]
    variable_type: VariableTypeDefinition,
}

/// [VariableDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub struct Variable {
    variable_type: VariableType,
}

impl VariableDefinition {
    /// Returns the corresponding [Variable] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> Variable {
        Variable {
            variable_type: self.variable_type.with_config(constraints),
        }
    }

    /// Returns the variable's declared type.
    pub fn kind(&self) -> &VariableTypeDefinition {
        &self.variable_type
    }
}

impl Variable {
    /// Builds a string variable already populated with its final value.
    pub fn new_final_string_variable(final_value: impl ToString) -> Self {
        Self {
            variable_type: VariableType::String(StringFields {
                inner: Fields {
                    required: false,
                    default: None,
                    final_value: Some(final_value.to_string()),
                },
                variants: Default::default(),
            }),
        }
    }

    /// Returns whether this variable must be provided with a value.
    pub fn is_required(&self) -> bool {
        self.variable_type.is_required()
    }

    /// Returns the variable's final value (its set value, or its default), if any.
    pub fn get_final_value(&self) -> Option<VariableValue> {
        self.variable_type.get_final_value()
    }

    /// Sets the variable's final value from the given YAML value, validating it against the type.
    pub fn merge_with_yaml_value(&mut self, yaml: serde_json::Value) -> Result<(), AgentTypeError> {
        self.variable_type.merge_with_yaml_value(yaml)
    }

    /// Returns the variable's type.
    pub fn kind(&self) -> &VariableType {
        &self.variable_type
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::agent_type::variable::{
        VariableDefinition,
        fields::{Fields, FieldsDefinition, StringFields, StringFieldsDefinition},
        tree::Tree,
        variable_type::{VariableType, VariableTypeDefinition},
        variants::VariantsConfig,
    };

    use super::Variable;

    impl From<Fields<serde_json::Value>> for Variable {
        fn from(kind_value: Fields<serde_json::Value>) -> Self {
            Self {
                variable_type: VariableType::Yaml(kind_value),
            }
        }
    }

    impl From<Fields<HashMap<String, String>>> for Variable {
        fn from(kind_value: Fields<HashMap<String, String>>) -> Self {
            Self {
                variable_type: VariableType::StringMap(kind_value),
            }
        }
    }

    impl Variable {
        pub(crate) fn new<T>(required: bool, default: Option<T>, final_value: Option<T>) -> Self
        where
            T: PartialEq,
            VariableType: From<Fields<T>>,
        {
            Self {
                variable_type: Fields::new(required, default, final_value).into(),
            }
        }

        pub(crate) fn new_string(
            required: bool,
            default: Option<String>,
            final_value: Option<String>,
        ) -> Self {
            Self {
                variable_type: StringFields::new(
                    required,
                    default,
                    Default::default(),
                    final_value,
                )
                .into(),
            }
        }
    }

    #[test]
    fn variable_definition_kind_returns_declared_type() {
        let variable_type = VariableTypeDefinition::String(StringFieldsDefinition {
            inner: FieldsDefinition {
                required: false,
                default: None,
            },
            variants: Default::default(),
        });
        let definition = VariableDefinition {
            variable_type: variable_type.clone(),
        };

        assert_eq!(definition.kind(), &variable_type);
    }

    #[test]
    fn variable_definition_tree_deserialize() {
        let value = r#"
foo:
  bar:
    var_name:
      type: string
      required: false
      default: "a"
      variants:
        ac_config_field: "foo.bar.var_name"
        values: ["a", "b"]
"#;
        let tree: Tree<VariableDefinition> = serde_saphyr::from_str(value).unwrap();
        let expected: Tree<VariableDefinition> = Tree::Mapping(HashMap::from([(
            "foo".to_string(),
            Tree::Mapping(HashMap::from([(
                "bar".to_string(),
                Tree::Mapping(HashMap::from([(
                    "var_name".to_string(),
                    Tree::End(VariableDefinition {
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
