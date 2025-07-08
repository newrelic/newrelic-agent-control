//! This module defines the fields the Agent Type supports depending on the corresponding type.
use std::{fmt::Debug, path::PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    variable::variants::{Variants, VariantsConfig},
};

/// Defines the fields supported by a Variable in an Agent Type
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct FieldsDefinition<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) variants: VariantsConfig<T>,
}

/// Type to also support a `file_path` for particular variable types.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FieldsWithPathDefinition<T>
where
    T: PartialEq,
{
    #[serde(flatten)]
    pub(crate) inner: FieldsDefinition<T>,
    pub(crate) file_path: PathBuf,
}

impl<T: PartialEq> FieldsWithPathDefinition<T> {
    pub fn with_config(self) -> FieldsWithPath<T> {
        FieldsWithPath {
            inner: self.inner.with_config(),
            file_path: self.file_path,
        }
    }
}

/// A [FieldsDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct Fields<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>,
    pub(crate) variants: Variants<T>,
}

/// A [FieldsWithPathDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct FieldsWithPath<T>
where
    T: PartialEq,
{
    #[serde(flatten)]
    pub(crate) inner: Fields<T>,
    pub(crate) file_path: PathBuf,
}

impl<T> FieldsDefinition<T>
where
    T: PartialEq,
{
    /// Returns the corresponding [Fields] according to the provided configuration.
    // TODO: actually receive configuration
    pub fn with_config(self) -> Fields<T> {
        let variants = self.variants.values; // TODO: variants.ac_config_field and get the right value
        Fields {
            required: self.required,
            default: self.default,
            final_value: None,
            variants,
        }
    }
}

impl<T> Fields<T>
where
    T: PartialEq + Debug,
{
    pub(crate) fn set_final_value(&mut self, value: T) -> Result<(), AgentTypeError> {
        if !self.variants.is_valid(&value) {
            return Err(AgentTypeError::InvalidVariant(
                format!("{value:?}"), // TODO: check if we may be exposing ${nr-env} values in this error
                self.variants.0.iter().map(|v| format!("{v:?}")).collect(),
            ));
        }
        self.final_value = Some(value);
        Ok(())
    }
}

impl<T> FieldsWithPath<T>
where
    T: PartialEq,
{
    pub(crate) fn get_file_path(&self) -> &PathBuf {
        &self.file_path
    }
    pub(crate) fn set_file_path(&mut self, path: PathBuf) {
        self.file_path = path;
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
            variants: Option<VariantsConfig<T>>,
            required: bool,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;
        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            return Err(D::Error::custom(AgentTypeError::MissingDefault));
        }

        Ok(FieldsDefinition {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
            variants: intermediate_spec
                .variants
                .unwrap_or(VariantsConfig::default()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Fields, FieldsWithPath};

    impl<T> Fields<T>
    where
        T: PartialEq,
    {
        pub(crate) fn new(required: bool, default: Option<T>, final_value: Option<T>) -> Self {
            Self {
                required,
                default,
                final_value,
                variants: Default::default(),
            }
        }
    }

    impl<T> FieldsWithPath<T>
    where
        T: PartialEq,
    {
        pub(crate) fn new(
            required: bool,
            default: Option<T>,
            final_value: Option<T>,
            file_path: PathBuf,
        ) -> Self {
            Self {
                inner: Fields {
                    required,
                    default,
                    final_value,
                    variants: Default::default(),
                },
                file_path,
            }
        }
    }

    #[test]
    fn test_set_final_value_valid_variant() {
        let mut fields: Fields<i32> = Fields {
            required: true,
            default: Some(1),
            final_value: None,
            variants: vec![1, 2, 3].into(),
        };

        assert!(fields.set_final_value(2).is_ok());
        assert_eq!(fields.final_value, Some(2));
    }

    #[test]
    fn test_set_final_value_invalid_variant() {
        let mut fields: Fields<i32> = Fields {
            required: true,
            default: Some(1),
            final_value: None,
            variants: vec![1, 2, 3].into(),
        };

        assert_eq!(
            fields.set_final_value(4).unwrap_err().to_string(),
            r#"Invalid variant provided as a value: `4`. Variants allowed: ["1", "2", "3"]"#
        );
        assert_eq!(fields.final_value, None);
    }

    #[test]
    fn test_set_final_value_no_variants() {
        let mut fields: Fields<i32> = Fields {
            required: true,
            default: Some(1),
            final_value: None,
            variants: Default::default(),
        };

        assert!(fields.set_final_value(2).is_ok());
        assert_eq!(fields.final_value, Some(2));
    }
}
