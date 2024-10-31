use std::{fmt::Debug, path::PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::agent_type::error::AgentTypeError;

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct KindValue<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>,
    pub(crate) variants: Vec<T>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct KindValueWithPath<T>
where
    T: PartialEq,
{
    #[serde(flatten)]
    pub(crate) inner: KindValue<T>,
    pub(crate) file_path: PathBuf,
}

impl<T> KindValue<T>
where
    T: PartialEq + Debug,
{
    pub(crate) fn set_final_value(&mut self, value: T) -> Result<(), AgentTypeError> {
        if !self.is_valid_variant(&value) {
            return Err(AgentTypeError::InvalidVariant(
                format!("{value:?}"),
                self.variants.iter().map(|v| format!("{v:?}")).collect(),
            ));
        }
        self.final_value = Some(value);
        Ok(())
    }
    pub(crate) fn is_valid_variant(&self, value: &T) -> bool {
        self.variants.is_empty() || self.variants.iter().any(|v| v == value)
    }
}

impl<T> KindValueWithPath<T>
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

impl<'de, T> Deserialize<'de> for KindValue<T>
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
        struct IntermediateValueKind<T> {
            default: Option<T>,
            variants: Option<Vec<T>>,
            required: bool,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;
        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            return Err(D::Error::custom(AgentTypeError::MissingDefault));
        }

        Ok(KindValue {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
            final_value: None,
            variants: intermediate_spec.variants.unwrap_or(Vec::new()),
        })
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::{KindValue, KindValueWithPath};

    impl<T> KindValue<T>
    where
        T: PartialEq,
    {
        pub(crate) fn new(required: bool, default: Option<T>, final_value: Option<T>) -> Self {
            Self {
                required,
                default,
                final_value,
                variants: Vec::new(),
            }
        }
    }

    impl<T> KindValueWithPath<T>
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
                inner: KindValue {
                    required,
                    default,
                    final_value,
                    variants: Vec::new(),
                },
                file_path,
            }
        }
    }

    #[test]
    fn test_set_final_value_valid_variant() {
        let mut kind_value: KindValue<i32> = KindValue {
            required: true,
            default: Some(1),
            final_value: None,
            variants: vec![1, 2, 3],
        };

        assert!(kind_value.set_final_value(2).is_ok());
        assert_eq!(kind_value.final_value, Some(2));
    }

    #[test]
    fn test_set_final_value_invalid_variant() {
        let mut kind_value: KindValue<i32> = KindValue {
            required: true,
            default: Some(1),
            final_value: None,
            variants: vec![1, 2, 3],
        };

        assert_eq!(
            kind_value.set_final_value(4).unwrap_err().to_string(),
            r#"Invalid variant provided as a value: `4`. Variants allowed: ["1", "2", "3"]"#
        );
        assert_eq!(kind_value.final_value, None);
    }

    #[test]
    fn test_set_final_value_no_variants() {
        let mut kind_value: KindValue<i32> = KindValue {
            required: true,
            default: Some(1),
            final_value: None,
            variants: Vec::new(),
        };

        assert!(kind_value.set_final_value(2).is_ok());
        assert_eq!(kind_value.final_value, Some(2));
    }
}
