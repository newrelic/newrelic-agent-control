use std::{collections::HashMap, mem};

use serde::Deserialize;

use crate::agent_type::trivial_value::TrivialValue;

/// Constraints that are loaded at startup and can be applied to agent type definitions.
/// The definition of a variable can be modified by these constraints if the agent type
/// references these. Hence, the constraints take the form of a key-value store.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct VariableConstraints {
    /// Accepted variants for a variable.
    /// These values of the `HashMap` are [`TrivialValue`]s, but all the elements of the `Vec` should
    /// be of the same type. This is validated when the config is loaded during AC startup.
    pub variants: Variants,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Variants(HashMap<String, Vec<TrivialValue>>);

impl<'de> Deserialize<'de> for Variants {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as DeError;
        HashMap::<String, Vec<TrivialValue>>::deserialize(deserializer)
            // Validate that all values in the map are of the same type.
            .and_then(|m| {
                m.values()
                    .all(|v| same_variant(v.iter()))
                    .then_some(m)
                    .ok_or(DeError::custom(
                        "All values in a `variants` key must be of the same type",
                    ))
            })
            .map(Variants)
    }
}

fn same_variant<'a>(mut values: impl Iterator<Item = &'a TrivialValue>) -> bool {
    values
        .next()
        .map(mem::discriminant)
        .is_none_or(|first| values.map(mem::discriminant).all(|v| v == first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::trivial_value::TrivialValue;
    use rstest::rstest;
    use serde_json::json;

    fn trivial_int(i: i64) -> TrivialValue {
        // TrivialValue does not have Integer, so use Number via serde_yaml::Number
        TrivialValue::from(serde_yaml::Number::from(i))
    }
    fn trivial_str(s: &str) -> TrivialValue {
        TrivialValue::from(s.to_string())
    }

    #[rstest]
    #[case::all_nums(vec![trivial_int(1), trivial_int(2), trivial_int(3)], true)]
    #[case::all_strs(vec![trivial_str("a"), trivial_str("b")], true)]
    #[case::mixed(vec![trivial_int(1), trivial_str("b")], false)]
    #[case::empty(vec![], true)]
    fn test_variants(#[case] values: Vec<TrivialValue>, #[case] expected: bool) {
        assert_eq!(expected, same_variant(values.iter()))
    }

    #[test]
    fn test_variants_deserialize_all_same_type() {
        let json = json!({
            "foo": [1, 2, 3],
            "bar": [4, 5]
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }

    #[test]
    fn test_variants_deserialize_mixed_types() {
        let json = json!({
            "foo": [1, "bar", 3]
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_err());
        let err = variants.unwrap_err().to_string();
        assert!(
            err.contains("All values in a `variants` key must be of the same type"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_variants_deserialize_empty_vec() {
        let json = json!({
            "foo": [],
            "bar": []
        });
        let variants: Result<Variants, _> = serde_json::from_value(json);
        assert!(variants.is_ok());
    }
}
