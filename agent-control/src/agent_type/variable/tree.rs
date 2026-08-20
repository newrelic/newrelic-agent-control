//! This module defines a tree to represent Agent Type variables.
//!
//! The tree structure is needed because variable names can be nested an arbitrary number of levels. Example:
//!
//! ```yaml
//! variables:
//!   linux:
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
use thiserror::Error;

use crate::agent_type::templates::TEMPLATE_KEY_SEPARATOR;
use crate::agent_type::variable::name::{VariableNameError, validate_variable_name};

/// This struct assures that variables have at least a name (one level of nested names).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VarTree<T>(pub(crate) HashMap<String, Tree<T>>);

/// A variable-name validation failure, with the dotted path context of the failing key.
#[derive(Error, Debug, PartialEq)]
#[error("invalid variable name '{path}' (segment '{segment}'): {source}")]
pub struct VariableNameTreeError {
    /// Full dotted path up to and including the offending segment.
    path: String,
    /// The raw map key that failed validation (may itself contain the separator).
    segment: String,
    #[source]
    source: VariableNameError,
}

/// Represents a Tree for an arbitrary type.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum Tree<T> {
    /// A leaf node holding a value of type `T`.
    End(T),
    /// An intermediate node mapping names to subtrees.
    Mapping(HashMap<String, Self>),
}

// We cannot use the 'derive' of default implementation because serde's Deserialize needs it explicit as T might not
// implement Default.
impl<T> Default for VarTree<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> VarTree<T> {
    /// Validates every key in the tree, at every nesting level, against variable-name rules.
    /// Structural only — does not require `T: Clone` (unlike [Self::flatten]).
    pub fn validate_names(&self) -> Result<(), VariableNameTreeError> {
        self.0
            .iter()
            .try_for_each(|(key, subtree)| Self::inner_validate(None, key, subtree))
    }

    fn inner_validate(
        parent_path: Option<&str>,
        key: &str,
        tree: &Tree<T>,
    ) -> Result<(), VariableNameTreeError> {
        let path = match parent_path {
            Some(p) => format!("{p}{TEMPLATE_KEY_SEPARATOR}{key}"),
            None => key.to_string(),
        };
        validate_variable_name(key).map_err(|source| VariableNameTreeError {
            path: path.clone(),
            segment: key.to_string(),
            source,
        })?;
        match tree {
            Tree::End(_) => Ok(()),
            Tree::Mapping(m) => m
                .iter()
                .try_for_each(|(k, v)| Self::inner_validate(Some(&path), k, v)),
        }
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
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::single_level(
        r#"
foo:
"#
    )]
    #[case::nested(
        r#"
common:
  two:
    three:
"#
    )]
    fn valid_trees_pass(#[case] yaml: &str) {
        let tree: VarTree<()> = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(tree.validate_names(), Ok(()));
    }

    #[rstest]
    #[case::top_level_leaf_key(
        r#"
"foo.bar":
"#,
        "foo.bar",
        "foo.bar",
        VariableNameError::InvalidCharacter('.')
    )]
    #[case::nested_three_levels_deep(
        r#"
common:
  two:
    "three:x":
"#,
        "common.two.three:x",
        "three:x",
        VariableNameError::InvalidCharacter(':')
    )]
    #[case::intermediate_mapping_key(
        r#"
"a.b":
  c:
"#,
        "a.b",
        "a.b",
        VariableNameError::InvalidCharacter('.')
    )]
    fn invalid_trees_are_rejected(
        #[case] yaml: &str,
        #[case] expected_path: &str,
        #[case] expected_segment: &str,
        #[case] expected_source: VariableNameError,
    ) {
        let tree: VarTree<()> = serde_saphyr::from_str(yaml).unwrap();
        let err = tree.validate_names().unwrap_err();
        assert_eq!(err.path, expected_path);
        assert_eq!(err.segment, expected_segment);
        assert_eq!(err.source, expected_source);
    }
}
