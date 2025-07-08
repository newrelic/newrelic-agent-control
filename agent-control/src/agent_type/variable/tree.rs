//! This module defines a tree to represent Agent Type variables.
//!
//! The tree structure is needed because variable names can be nested an arbitrary number of levels. Example:
//!
//! ```yaml
//! variables:
//!   common:
//!     foo:
//!       bar:
//!         variable_name:
//!           description: "Some description"
//!           required: true
//!           type: string
//! ```
//! The variables can be referenced with [TEMPLATE_KEY_SEPARATOR] separating names levels. The example variable from above could be used
//! in agent types as `${nr-var:foo.bar.variable_name}`.

use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::{error::AgentTypeError, templates::TEMPLATE_KEY_SEPARATOR};

/// This struct assures that variables have at least a name (one level of nested names).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VarTree<T>(pub(crate) HashMap<String, Tree<T>>);

/// Represents a Tree for an arbitrary type.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum Tree<T> {
    End(T),
    Mapping(HashMap<String, Self>),
}

// We cannot use the 'derive' of default implementation because serde's Deserialize needs it explicit as T might not
// implement Default.
impl<T> Default for VarTree<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T: Clone> VarTree<T> {
    /// Returns a [HashMap] representing the _flatten_ variables. Each variable key will be the path of the variable
    /// in the tree separated by [TEMPLATE_KEY_SEPARATOR].
    pub fn flatten(self) -> HashMap<String, T> {
        self.0
            .into_iter()
            .flat_map(|(k, v)| Self::inner_flatten(k, v))
            .collect()
    }

    /// Merges the current [VarTree] with another, returning an error if any key overlaps.
    pub fn merge(self, variables: Self) -> Result<Self, AgentTypeError> {
        Ok(Self(
            Self::merge_inner(&self.0, &variables.0)
                .map_err(AgentTypeError::ConflictingVariableDefinition)?,
        ))
    }

    /// Merges recursively two inner hashmaps if there is no conflicting key.
    ///
    /// # Errors
    ///
    /// This function will return an String error containing the full path of the first conflicting key if any
    /// [VariableDefinitionTree::End] overlaps.
    fn merge_inner(
        a: &HashMap<String, Tree<T>>,
        b: &HashMap<String, Tree<T>>,
    ) -> Result<HashMap<String, Tree<T>>, String> {
        let mut merged = a.clone();
        for (key, value) in b {
            match (merged.get(key), value) {
                // Include the value when its key doesn't overlap.
                (None, _) => {
                    merged.insert(key.into(), value.clone());
                }
                // Merge overlapping mappings.
                (Some(Tree::Mapping(inner_a)), Tree::Mapping(inner_b)) => {
                    let merged_inner = Self::merge_inner(inner_a, inner_b)
                        .map_err(|err| format!("{key}.{err}"))?;
                    merged.insert(key.clone(), Tree::Mapping(merged_inner));
                }
                // Any other option implies an overlapping end (conflicting key).
                (Some(_), _) => return Err(key.into()),
            }
        }
        Ok(merged)
    }

    /// Helper for [Self::flatten] implementation.
    fn inner_flatten(key: String, spec: Tree<T>) -> HashMap<String, T> {
        let mut result = HashMap::new();
        match spec {
            Tree::End(s) => _ = result.insert(key, s),
            Tree::Mapping(m) => m.into_iter().for_each(|(k, v)| {
                result.extend(Self::inner_flatten(
                    key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                    v,
                ))
            }),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_merge_trees() {
        let a = r#"
config:
  general: "general"
common: "common"
        "#;

        let b = r#"
config:
  specific: "specific"
env:
  key: "value"
        "#;

        let expected = r#"
config:
  general: "general"
  specific: "specific"
common: "common"
env:
  key: "value"
        "#;

        let a: VarTree<String> = serde_yaml::from_str(a).unwrap();
        let b: VarTree<String> = serde_yaml::from_str(b).unwrap();
        let expected: VarTree<String> = serde_yaml::from_str(expected).unwrap();

        assert_eq!(expected, a.merge(b).unwrap());
    }

    #[rstest]
    #[case::conflicting_leaves(
        r#"
config:
  general: a
        "#,
        r#"
config:
  general: b
        "#,
        "config.general"
    )]
    #[case::conflicting_branch_leave(
        r#"
var:
  nested: nested
        "#,
        r#"
var: var
        "#,
        "var"
    )]
    #[case::conflicting_leave_branch(
        r#"
var: var
        "#,
        r#"
var:
  nested: nested
        "#,
        "var"
    )]
    #[case::conflicting_branch_and_leave_nested(
        r#"
var:
  several:
    nested:
      levels: value
        "#,
        r#"
var:
  several:
    nested: nested
        "#,
        "var.several.nested"
    )]

    fn test_merge_variable_tree_errors(
        #[case] a: &str,
        #[case] b: &str,
        #[case] conflicting_key: &str,
    ) {
        let a: VarTree<String> = serde_yaml::from_str(a).unwrap();
        let b: VarTree<String> = serde_yaml::from_str(b).unwrap();
        let result = a.merge(b);
        assert_matches!(result, Err(AgentTypeError::ConflictingVariableDefinition(k)) => {
            assert_eq!(k, conflicting_key);
        })
    }
}
