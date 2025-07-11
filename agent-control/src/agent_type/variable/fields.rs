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
}

/// Type support additional fields for the string type
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct StringFieldsDefinition {
    #[serde(flatten)]
    pub(crate) inner: FieldsDefinition<String>,
    #[serde(default = "Default::default")]
    pub(crate) variants: VariantsConfig<String>,
}

/// Type support a `file_path` field for particular variable types.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FieldsWithPathDefinition<T>
where
    T: PartialEq,
{
    #[serde(flatten)]
    pub(crate) inner: FieldsDefinition<T>,
    pub(crate) file_path: PathBuf,
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
}

/// A [StringFieldsDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct StringFields {
    #[serde(flatten)]
    pub(crate) inner: Fields<String>,
    pub(crate) variants: Variants<String>,
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
    /// TODO: add config
    pub fn with_config(self) -> Fields<T> {
        Fields {
            required: self.required,
            default: self.default,
            final_value: None,
        }
    }
}

impl StringFieldsDefinition {
    pub fn with_config(self) -> StringFields {
        let variants = self.variants.values; // TODO
        StringFields {
            inner: self.inner.with_config(),
            variants,
        }
    }
}

impl<T: PartialEq> FieldsWithPathDefinition<T> {
    pub fn with_config(self) -> FieldsWithPath<T> {
        FieldsWithPath {
            inner: self.inner.with_config(),
            file_path: self.file_path,
        }
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
            return Err(AgentTypeError::InvalidVariant(
                format!("{value:?}"), // TODO: check if we may be exposing ${nr-env} values in this error
                self.variants.0.iter().map(|v| format!("{v:?}")).collect(),
            ));
        }
        self.inner.set_final_value(value)?;
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
            required: bool,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;
        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            return Err(D::Error::custom(AgentTypeError::MissingDefault));
        }

        Ok(FieldsDefinition {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use assert_matches::assert_matches;

    use crate::agent_type::{
        error::AgentTypeError,
        variable::{fields::StringFields, variants::Variants},
    };

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
                },
                file_path,
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
        assert_matches!(result, AgentTypeError::InvalidVariant(_, _));

        assert_eq!(fields.inner.final_value, None);
    }

    #[test]
    fn test_set_final_value_no_variants() {
        let mut fields = StringFields::new(true, Some("a".into()), Default::default(), None);

        assert!(fields.set_final_value("b".into()).is_ok());
        assert_eq!(fields.inner.final_value, Some("b".into()));
    }
}
