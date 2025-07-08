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
        .is_none_or(|first| values.all(|v| mem::discriminant(v) == first))
}
