use std::collections::HashMap;

use serde::Deserialize;
use serde_yaml::Number;

/// Constraints that are loaded at startup and can be applied to agent type definitions.
/// The definition of a variable can be modified by these constraints if the agent type
/// references these. Hence, the constraints take the form of a key-value store.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct VariableConstraints {
    /// Accepted variants for a variable.
    pub variants: Variants,
}

/// Definition of variant lists by key. The values are collections of elements of the same type.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Variants(HashMap<String, TypedCollection>);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
#[serde(expecting = "expected a collection of elements of the same type (number, string, bool)")]
pub enum TypedCollection {
    Numbers(Vec<Number>),
    Strings(Vec<String>),
    Bools(Vec<bool>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_variants_same_type() {
        let json = json!({
            "foo": [1, 2, 3],
            "bar": [4, 5]
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_mixed_types_should_fail() {
        let json = json!({
            "foo": [1, "bar", 3]
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_err());
        let err = variants.unwrap_err().to_string();
        assert!(
            err.contains(
                "expected a collection of elements of the same type (number, string, bool)"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialize_variants_empty() {
        let json = json!({
            "foo": [],
            "bar": []
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_supported_types() {
        let json = json!({
            "foo": [1, 2, 3],
            "bar": ["a", "b", "c"],
            "baz": [true, false]
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn deserialize_variants_invalid_type() {
        let json = json!({
            "foo": [{ "key": "value" }] // a list of objects is not a valid type
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_err());
        let err = variants.unwrap_err().to_string();
        assert!(
            err.contains(
                "expected a collection of elements of the same type (number, string, bool)"
            ),
            "unexpected error: {err}"
        );
    }
}
