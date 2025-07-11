use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::variable::variants::Variants;

/// Constraints that are loaded at startup and can be applied to agent type definitions.
/// The definition of a variable can be modified by these constraints if the agent type
/// references these. Hence, the constraints take the form of a key-value store.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct VariableConstraints {
    /// Accepted variants for variables.
    pub variants: VariantsConstraints,
}

/// Definition of variant lists by key. The values are collections of elements of the same type.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct VariantsConstraints(HashMap<String, SupportedValues>);

impl VariantsConstraints {
    pub fn get(&self, key: &str) -> Option<&SupportedValues> {
        self.0.get(key)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
/// Represents the collection of supported values for a particular variable
pub struct SupportedValues(Vec<String>);

impl From<&SupportedValues> for Variants<String> {
    fn from(value: &SupportedValues) -> Self {
        Variants::from(value.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_variants_same_type() {
        let json = json!({
            "foo": ["1", "2", "3"],
            "bar": ["4", "5"]
        });
        let variants: Result<VariantsConstraints, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_mixed_types_should_fail() {
        let json = json!({
            "foo": [1, "bar", 3]
        });
        let variants: Result<VariantsConstraints, _> = serde_json::from_value(json);
        assert!(variants.is_err());
        let err = variants.unwrap_err().to_string();
        assert!(err.contains("expected a string"), "unexpected error: {err}");
    }

    #[test]
    fn deserialize_variants_empty() {
        let json = json!({
            "foo": [],
            "bar": []
        });
        let variants: Result<VariantsConstraints, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_supported_types() {
        let json = json!({
            "bar": ["a", "b", "c"],
        });
        let variants: Result<VariantsConstraints, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_invalid_type() {
        let json = json!({
            "foo": [{ "key": "value" }] // a list of objects is not a valid type
        });
        let variants: Result<VariantsConstraints, _> = serde_json::from_value(json);
        assert!(variants.is_err());
        let err = variants.unwrap_err().to_string();
        assert!(err.contains("expected a string"), "unexpected error: {err}");
    }
}
