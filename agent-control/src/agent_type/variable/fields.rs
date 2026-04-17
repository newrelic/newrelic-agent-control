//! This module defines the fields the Agent Type supports depending on the corresponding type.
use std::fmt::Debug;

use serde::{Deserialize, Deserializer, Serialize};
use tracing::debug;

use crate::agent_type::{
    error::AgentTypeError,
    variable::{
        constraints::{VariableConstraints, VariantsConstraints},
        variants::{Variants, VariantsConfig},
    },
};

/// Defines the fields supported by a Variable in an Agent Type
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct FieldsDefinition<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
}

/// Type to support special default deserialization for 'null' Yaml value in 'default'.
/// This type is special since the only way to specify the Yaml null value in the AgentType
/// which is rendered from Yaml is by setting default to null which is equal to not define it.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct YamlFieldsDefinition {
    #[serde(flatten)]
    pub(crate) inner: FieldsDefinition<serde_yaml::Value>,
}

/// Type support additional fields for the string type
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct StringFieldsDefinition {
    #[serde(flatten)]
    pub(crate) inner: FieldsDefinition<String>,
    #[serde(default = "Default::default")]
    pub(crate) variants: VariantsConfig<String>,
}

/// A [FieldsDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct Fields<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>, // TODO: move this outside the struct and avoid mutating the variables
}

/// A [StringFieldsDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct StringFields {
    #[serde(flatten)]
    pub(crate) inner: Fields<String>,
    pub(crate) variants: Variants<String>,
}

impl<T> FieldsDefinition<T>
where
    T: PartialEq,
{
    /// Returns the corresponding inner [Fields].
    pub fn with_config(self, _: &VariableConstraints) -> Fields<T> {
        Fields {
            required: self.required,
            default: self.default,
            final_value: None,
        }
    }
}

impl YamlFieldsDefinition {
    /// Returns the corresponding inner [Fields].
    pub fn with_config(self, _: &VariableConstraints) -> Fields<serde_yaml::Value> {
        Fields {
            required: self.inner.required,
            default: self.inner.default,
            final_value: None,
        }
    }
}

impl StringFieldsDefinition {
    /// Returns the corresponding [StringFields] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> StringFields {
        let variants = self.build_variants(&constraints.variants);
        StringFields {
            inner: self.inner.with_config(constraints),
            variants,
        }
    }

    /// Builds the set of valid variants as configured, considering the constraints configuration provided.
    fn build_variants(&self, variants_constraints: &VariantsConstraints) -> Variants<String> {
        let Some(ac_config_field) = self.variants.ac_config_field.as_ref() else {
            return self.variants.values.clone();
        };

        let Some(supported_values) = variants_constraints.get(ac_config_field) else {
            debug!(%ac_config_field,
                "The variants pointed in Agent Type are not set in Agent Control configuration, using defaults"
            );
            return self.variants.values.clone();
        };

        supported_values.into()
    }
}

impl<T> Fields<T>
where
    T: PartialEq + Debug,
{
    pub(crate) fn set_final_value(&mut self, value: T) -> Result<(), AgentTypeError> {
        self.final_value = Some(value);
        Ok(())
    }
}

impl StringFields {
    pub(crate) fn set_final_value(&mut self, value: String) -> Result<(), AgentTypeError> {
        if !self.variants.is_valid(&value) {
            return Err(AgentTypeError::InvalidVariant(self.variants.to_string()));
        }
        self.inner.set_final_value(value)?;
        Ok(())
    }
}

impl<'de, T> Deserialize<'de> for FieldsDefinition<T>
where
    T: Deserialize<'de> + PartialEq,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        // intermediate serialization type to validate `default` and `required` fields
        #[derive(Debug, Deserialize)]
        struct IntermediateValueKind<T: PartialEq> {
            default: Option<T>,
            required: bool,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;

        if intermediate_spec.required && intermediate_spec.default.is_some() {
            return Err(D::Error::custom(AgentTypeError::Parse(
                "default value cannot be specified for a required spec key".to_string(),
            )));
        }

        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            return Err(D::Error::custom(AgentTypeError::Parse(
                "missing default value for a non-required spec key".to_string(),
            )));
        }

        Ok(FieldsDefinition {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
        })
    }
}

// An special deserializer is used in order to consider the absence of default as a 'null' Yaml default value.
impl<'de> Deserialize<'de> for YamlFieldsDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        #[derive(Debug, Deserialize)]
        struct IntermediateValueKind {
            default: Option<serde_yaml::Value>,
            required: bool,
        }

        let mut intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;

        if intermediate_spec.required && intermediate_spec.default.is_some() {
            return Err(D::Error::custom(AgentTypeError::Parse(
                "default value cannot be specified for a required spec key".to_string(),
            )));
        }

        // Supports to set 'Null' Yaml default value.
        if !intermediate_spec.required && intermediate_spec.default.is_none() {
            intermediate_spec.default = Some(serde_yaml::Value::Null)
        }

        Ok(YamlFieldsDefinition {
            inner: FieldsDefinition {
                required: intermediate_spec.required,
                default: intermediate_spec.default,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use rstest::rstest;
    use serde_yaml::Mapping;

    use crate::agent_type::{
        error::AgentTypeError,
        variable::{
            constraints::VariableConstraints,
            fields::{
                FieldsDefinition, StringFields, StringFieldsDefinition, YamlFieldsDefinition,
            },
            variants::Variants,
        },
    };

    use super::Fields;

    impl<T> Fields<T>
    where
        T: PartialEq,
    {
        pub(crate) fn new(required: bool, default: Option<T>, final_value: Option<T>) -> Self {
            Self {
                required,
                default,
                final_value,
            }
        }
    }

    impl StringFields {
        pub(crate) fn new(
            required: bool,
            default: Option<String>,
            variants: Variants<String>,
            final_value: Option<String>,
        ) -> Self {
            Self {
                inner: Fields::<String> {
                    required,
                    default,
                    final_value,
                },
                variants,
            }
        }
    }

    impl YamlFieldsDefinition {
        pub(crate) fn new(required: bool, default: Option<serde_yaml::Value>) -> Self {
            Self {
                inner: FieldsDefinition { required, default },
            }
        }
    }

    #[test]
    fn test_set_final_value_valid_variant() {
        let mut fields = StringFields::new(
            true,
            Some("a".into()),
            vec!["a".to_string(), "b".to_string(), "c".to_string()].into(),
            None,
        );

        assert!(fields.set_final_value("b".into()).is_ok());
        assert_eq!(fields.inner.final_value, Some("b".into()));
    }

    #[test]
    fn test_set_final_value_invalid_variant() {
        let mut fields = StringFields::new(
            true,
            Some("a".into()),
            vec!["a".to_string(), "b".to_string(), "c".to_string()].into(),
            None,
        );
        let result = fields.set_final_value("d".into()).unwrap_err();
        assert_matches!(result, AgentTypeError::InvalidVariant(_));

        assert_eq!(fields.inner.final_value, None);
    }

    #[test]
    fn test_set_final_value_no_variants() {
        let mut fields = StringFields::new(true, Some("a".into()), Default::default(), None);

        assert!(fields.set_final_value("b".into()).is_ok());
        assert_eq!(fields.inner.final_value, Some("b".into()));
    }

    #[rstest]
    #[case::no_variants_default(
        r#"{"required": true}"#,
        r#"{"variants": {}}"#,
        StringFields::new(true, None, Default::default(), None)
    )]
    #[case::variants_with_no_match_with_no_values(
        r#"{"required": true, "variants": {"ac_config_field": "some_key"}}"#,
        r#"{"variants": {"other_key": ["a", "b"]}}"#,
        StringFields::new(true, None, Default::default(), None)
    )]
    #[case::variants_with_no_match_with_values(
        r#"{"required": true, "variants": {"ac_config_field": "some_key", "values": ["x"]}}"#,
        r#"{"variants": {"other_key": ["a", "b"]}}"#,
        StringFields::new(true, None, vec!["x".to_string()].into(), None)
    )]
    #[case::variants_with_match_with_no_values(
        r#"{"required": true, "variants": {"ac_config_field": "some_key"}}"#,
        r#"{"variants": {"some_key": ["a", "b"]}}"#,
        StringFields::new(true, None, vec!["a".to_string(), "b".to_string()].into(), None)
    )]
    #[case::variants_with_match_with_values(
        r#"{"required": true, "variants": {"ac_config_field": "some_key", "values": ["x"]}}"#,
        r#"{"variants": {"some_key": ["a", "b"]}}"#,
        StringFields::new(true, None, vec!["a".to_string(), "b".to_string()].into(), None)
    )]

    fn test_string_field_with_config(
        #[case] def_str: &str,
        #[case] constraints: &str,
        #[case] expected: StringFields,
    ) {
        let fields_def: StringFieldsDefinition = serde_json::from_str(def_str).unwrap();
        let constraints: VariableConstraints = serde_json::from_str(constraints).unwrap();

        let fields = fields_def.with_config(&constraints);
        assert_eq!(fields, expected);
    }

    #[rstest]
    #[case::null_explicit_title(
        r#"
        required: false
        default: Null
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::null_explicit_lower(
        r#"
        required: false
        default: null
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::null_explicit_upper(
        r#"
        required: false
        default: NULL
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::null_explicit_symbol(
        r#"
        required: false
        default: ~
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::null_explicit_empty(
        r#"
        required: false
        default:
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::null_by_absence(
        r#"
        required: false
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Null),)
    )]
    #[case::emtpy_map(
        r#"
        required: false
        default: { }
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Mapping(Mapping::default())),)
    )]
    #[case::other_yaml_value(
        r#"
        required: false
        default: true
        "#,
        YamlFieldsDefinition::new(false, Some(serde_yaml::Value::Bool(true)),)
    )]
    fn test_parse_yaml_field_definition(
        #[case] def_str: &str,
        #[case] expected: YamlFieldsDefinition,
    ) {
        let fields_def: YamlFieldsDefinition = serde_yaml::from_str(def_str).unwrap();
        assert_eq!(fields_def, expected);
    }

    #[rstest]
    #[case::missing_required(
        r#"
        default: true
        "#,
        "missing"
    )]
    #[case::required_default(
        r#"
        required: true
        default: true
        "#,
        "default value cannot be specified for a required spec key"
    )]
    fn test_fail_parse_yaml_field_definition(#[case] def_str: &str, #[case] expected_error: &str) {
        let err = serde_yaml::from_str::<YamlFieldsDefinition>(def_str).unwrap_err();
        assert!(err.to_string().contains(expected_error));
    }
}
