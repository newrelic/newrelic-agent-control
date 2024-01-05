use std::path::PathBuf;

use serde::{Deserialize, Deserializer};

use crate::config::agent_type::error::AgentTypeError;

#[derive(Debug, PartialEq, Clone)]
pub struct KindValue<T> {
    pub(crate) required: bool,
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>,
    pub(crate) file_path: Option<PathBuf>, // Appropriate here? Or to FilePathWithContent?
    // pub(crate) variants: Option<Vec<T>>,
}

impl<T> KindValue<T> {
    pub(crate) fn not_required_without_default(&self) -> bool {
        !self.required && self.default.is_none()
    }
    pub(crate) fn set_default_as_final(&mut self) {
        self.final_value = self.default.take();
    }
}

impl<'de, T> Deserialize<'de> for KindValue<T>
where
    T: Deserialize<'de>,
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
            // variants: Option<Vec<T>>,
            required: bool,
            file_path: Option<PathBuf>,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;
        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            return Err(D::Error::custom(AgentTypeError::MissingDefault));
        }

        Ok(KindValue {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
            final_value: None,
            // variants: intermediate_spec.variants,
            file_path: intermediate_spec.file_path,
        })
    }
}
