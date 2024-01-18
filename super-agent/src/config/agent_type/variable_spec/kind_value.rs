use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::config::agent_type::error::AgentTypeError;

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct KindValue<T>
where
    T: PartialEq,
{
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>,
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
    T: PartialEq,
{
    pub(crate) fn set_final_value(&mut self, value: T) {
        self.final_value = Some(value);
    }
    pub(crate) fn get_final_value(&self) -> Option<&T> {
        self.final_value.as_ref()
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
                },
                file_path,
            }
        }
    }
}
