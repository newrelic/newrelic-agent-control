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
//!           required: true
//!           type: string
//! ```
//! The variables can be referenced with [TEMPLATE_KEY_SEPARATOR] separating names levels. The example variable from above could be used
//! in agent types as `${nr-var:foo.bar.variable_name}`.

use crate::agent_type::definition::YAMLConfig;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::TEMPLATE_KEY_SEPARATOR;
use crate::agent_type::variable::VariableDefinition;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::name::{VariableNameError, validate_variable_name};
use crate::agent_type::variable::namespace::{Namespace, VariableName};
use crate::agent_type::variable::value::VariableValues;
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;
use tracing::warn;

/// This struct assures that variables have at least a name (one level of nested names).
#[derive(Default, Clone, Debug, Deserialize, PartialEq)]
pub struct VariableTree(pub(crate) HashMap<String, VariableTreeNode>);

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
pub enum VariableTreeNode {
    /// A leaf node holding a value of type `T`.
    End(VariableDefinition),
    /// An intermediate node mapping names to subtrees.
    Mapping(HashMap<String, Self>),
}

impl VariableTree {
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
        tree: &VariableTreeNode,
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
            VariableTreeNode::End(_) => Ok(()),
            VariableTreeNode::Mapping(m) => m
                .iter()
                .try_for_each(|(k, v)| Self::inner_validate(Some(&path), k, v)),
        }
    }

    /// Returns a [HashMap] representing the _flatten_ variables. Each variable key will be the path of the variable
    /// in the tree separated by [TEMPLATE_KEY_SEPARATOR].
    pub fn flatten(self) -> HashMap<String, VariableDefinition> {
        self.0
            .into_iter()
            .flat_map(|(k, v)| Self::inner_flatten(k, v))
            .collect()
    }

    /// Helper for [Self::flatten] implementation.
    fn inner_flatten(key: String, spec: VariableTreeNode) -> HashMap<String, VariableDefinition> {
        let mut result = HashMap::new();
        match spec {
            VariableTreeNode::End(s) => _ = result.insert(key, s),
            VariableTreeNode::Mapping(m) => m.into_iter().for_each(|(k, v)| {
                result.extend(Self::inner_flatten(
                    key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                    v,
                ))
            }),
        }
        result
    }

    /// Resolves every definition in the tree into a fully-populated [`VariableValues`], using the
    /// provided constraints and user values.
    ///
    /// Errors when a required variable has no user value, when a user value doesn't match the declared type,
    /// or when a user value fails variants validation. User-config keys with no matching
    /// definition are logged as `WARN` and ignored.
    pub fn resolve(
        self,
        constraints: &VariableConstraints,
        user_values: YAMLConfig,
    ) -> Result<VariableValues, AgentTypeError> {
        let (resolved, mut missing) =
            resolve_sub_tree(user_values.into(), self.0, constraints, "")?;
        if !missing.is_empty() {
            missing.sort();
            return Err(AgentTypeError::ValuesNotPopulated(missing));
        }

        Ok(resolved)
    }
}

fn resolve_sub_tree(
    mut values: HashMap<String, serde_json::Value>,
    sub_tree: HashMap<String, VariableTreeNode>,
    constraints: &VariableConstraints,
    path_prefix: &str,
) -> Result<(VariableValues, Vec<String>), AgentTypeError> {
    let mut resolved: VariableValues = HashMap::new();
    let mut missing: Vec<String> = Vec::new();

    for (key, subtree) in sub_tree.into_iter() {
        let partial_variable_path = prefixed_path(path_prefix, &key);
        let variable_name = VariableName::new(Namespace::Variable, &partial_variable_path);
        let user_value = values.remove(&key);
        match subtree {
            VariableTreeNode::End(def) => {
                match def.resolve_value(constraints, user_value)? {
                    Some(value) => {
                        resolved.insert(variable_name, value);
                    }
                    // Missing required variables are accumulated so we surface every one at once
                    // instead of short-circuiting on the first.
                    None => missing.push(partial_variable_path),
                }
            }
            VariableTreeNode::Mapping(children) => {
                let inner: HashMap<String, serde_json::Value> = match user_value {
                    Some(v) => serde_json::from_value(v)?,
                    None => HashMap::new(),
                };
                let (child_resolved, child_missing) =
                    resolve_sub_tree(inner, children, constraints, &partial_variable_path)?;
                resolved.extend(child_resolved);
                missing.extend(child_missing);
            }
        }
    }
    for k in values.keys() {
        warn!(key = %prefixed_path(path_prefix, k), "Unexpected variable in the configuration");
    }
    Ok((resolved, missing))
}

fn prefixed_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}.{segment}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::variable::namespace::{Namespace, VariableName};
    use crate::agent_type::variable::value::{VariableType, VariableValue};
    use assert_matches::assert_matches;
    use rstest::rstest;
    use serde_json::json;

    #[test]
    fn flatten_joins_paths_with_dot_and_preserves_definitions() {
        let tree: VariableTree = serde_json::from_value(json!({
            "top": {"type": "string", "required": false, "default": "t"},
            "a": {
                "b": {
                    "c": {"type": "bool", "required": true},
                },
            },
        }))
        .unwrap();

        let flat = tree.flatten();

        let mut keys: Vec<_> = flat.keys().cloned().collect();
        keys.sort();

        assert_eq!(keys, vec!["a.b.c".to_string(), "top".to_string()]);
        assert_eq!(
            flat.get("a.b.c"),
            Some(&VariableDefinition {
                default: None,
                variants: None,
                variable_type: VariableType::Bool,
            })
        );
        assert_eq!(
            flat.get("top"),
            Some(&VariableDefinition {
                default: Some(VariableValue::String("t".to_string())),
                variants: None,
                variable_type: VariableType::String,
            })
        );
    }

    #[test]
    fn resolve_ignores_unknown_user_keys() {
        let tree: VariableTree = serde_json::from_value(json!({
            "known": {"type": "string", "required": true},
        }))
        .unwrap();
        let user: YAMLConfig =
            serde_json::from_value(json!({"known": "v", "unknown": "ignored"})).unwrap();

        let resolved = tree.resolve(&VariableConstraints::default(), user).unwrap();

        assert_eq!(resolved.len(), 1);
        assert!(resolved.contains_key(&VariableName::new(Namespace::Variable, "known")));
    }

    #[rstest]
    #[case::wrong_scalar_type(
        json!({
            "n": {"type": "bool", "required": true},
        }),
        json!({"n": "not a bool"}),
    )]
    #[case::scalar_where_mapping_expected(
        json!({
            "n": {
                "child": {"type": "string", "required": true},
            },
        }),
        json!({"n": "not a map"}),
    )]
    fn resolve_rejects_user_value_that_cannot_be_coerced(
        #[case] spec: serde_json::Value,
        #[case] user: serde_json::Value,
    ) {
        let tree: VariableTree = serde_json::from_value(spec).unwrap();
        let user: YAMLConfig = serde_json::from_value(user).unwrap();

        let err = tree
            .resolve(&VariableConstraints::default(), user)
            .unwrap_err();

        assert_matches!(err, AgentTypeError::ValueConversion(_));
    }

    #[rstest]
    #[case::in_variants("a", true)]
    #[case::not_in_variants("c", false)]
    fn resolve_enforces_configured_variants(#[case] user_value: &str, #[case] accepted: bool) {
        let tree: VariableTree = serde_json::from_value(json!({
            "name": {
                "type": "string",
                "required": true,
                "variants": {"values": ["a", "b"]},
            },
        }))
        .unwrap();
        let user: YAMLConfig = serde_json::from_value(json!({"name": user_value})).unwrap();

        let result = tree.resolve(&VariableConstraints::default(), user);

        if accepted {
            assert_eq!(
                result
                    .unwrap()
                    .get(&VariableName::new(Namespace::Variable, "name")),
                Some(&VariableValue::String(user_value.to_string()))
            );
        } else {
            assert_matches!(result, Err(AgentTypeError::InvalidVariant(_)));
        }
    }

    #[test]
    fn resolve_complex_tree_populates_variables_and_reports_missing_with_dot_notation() {
        // Multi-depth tree mixing required and optional variables with defaults.
        let tree: VariableTree = serde_json::from_value(json!({
            "service": {
                "host": {"type": "string", "required": true},
                "port": {"type": "number", "required": true},
                "metadata": {
                    "region": {"type": "string", "required": true},
                    "env": {"type": "string", "required": false, "default": "prod"},
                },
            },
            "logging": {
                "level": {"type": "string", "required": false, "default": "info"},
            },
            "debug": {"type": "bool", "required": true},
        }))
        .unwrap();

        // Partial values: two required variables at different depths are absent.
        let partial: YAMLConfig = serde_json::from_value(json!({
            "service": {"host": "example.com"},
            "debug": true,
        }))
        .unwrap();

        let err = tree
            .clone()
            .resolve(&VariableConstraints::default(), partial)
            .unwrap_err();

        assert_matches!(
            err,
            AgentTypeError::ValuesNotPopulated(paths)
                if paths == vec![
                    "service.metadata.region".to_string(),
                    "service.port".to_string(),
                ]
        );

        // All required provided; `logging.level` also provided, overriding its default;
        // `service.metadata.env` omitted, so its default fires.
        let full: YAMLConfig = serde_json::from_value(json!({
            "service": {
                "host": "example.com",
                "port": 9000,
                "metadata": {"region": "us-east-1"},
            },
            "logging": {"level": "debug"},
            "debug": true,
        }))
        .unwrap();

        let resolved = tree.resolve(&VariableConstraints::default(), full).unwrap();

        let expected: VariableValues = HashMap::from([
            (
                VariableName::new(Namespace::Variable, "service.host"),
                VariableValue::String("example.com".to_string()),
            ),
            (
                VariableName::new(Namespace::Variable, "service.port"),
                VariableValue::Number(9000_i64.into()),
            ),
            (
                VariableName::new(Namespace::Variable, "service.metadata.region"),
                VariableValue::String("us-east-1".to_string()),
            ),
            (
                VariableName::new(Namespace::Variable, "service.metadata.env"),
                VariableValue::String("prod".to_string()),
            ),
            (
                VariableName::new(Namespace::Variable, "logging.level"),
                VariableValue::String("debug".to_string()),
            ),
            (
                VariableName::new(Namespace::Variable, "debug"),
                VariableValue::Bool(true),
            ),
        ]);
        assert_eq!(resolved, expected);
    }
}
