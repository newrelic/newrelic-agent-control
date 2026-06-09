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

use crate::agent_type::templates::TEMPLATE_KEY_SEPARATOR;

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
