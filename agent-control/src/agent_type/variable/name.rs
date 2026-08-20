//! Validation for a single Agent Type variable-name tree key (one segment of a variable path,
//! not a dotted path as a whole).
//!
//! Variable names are kept clear of characters that already have structural meaning elsewhere
//! in the Agent Type grammar: `.` is [TEMPLATE_KEY_SEPARATOR](crate::agent_type::templates::TEMPLATE_KEY_SEPARATOR)
//! (tree-flattening / `${nr-var:a.b}` references) and `:` is
//! [Namespace::PREFIX_NS_SEPARATOR](crate::agent_type::variable::namespace::Namespace::PREFIX_NS_SEPARATOR)
//! (the `${nr-xxx:...}` namespace prefix separator). Allowing either in a variable name would make
//! it structurally indistinguishable from a nested path or a namespace-prefixed reference once
//! flattened.

use thiserror::Error;

pub(crate) const VARIABLE_NAME_MAX_LENGTH: usize = 64;

/// Errors describing why a variable-name segment is not valid.
#[derive(Error, Debug, PartialEq)]
pub enum VariableNameError {
    /// The name is empty.
    #[error("must not be empty")]
    Empty,
    /// The name exceeds the maximum allowed length.
    #[error("must be at most {max} characters, but it is {length}")]
    TooLong {
        /// The actual length of the name.
        length: usize,
        /// The maximum allowed length.
        max: usize,
    },
    /// The name does not start with an ASCII letter.
    #[error("must start with an ASCII letter")]
    InvalidStart,
    /// The name contains a character outside the allowed set.
    #[error(
        "contains invalid character '{0}', only ASCII letters, digits, '_' and '-' are allowed"
    )]
    InvalidCharacter(char),
}

/// Validates a single variable-name tree key (one path segment). Rejects `.` (the template key
/// separator) and `:` (the namespace separator), and more generally anything outside
/// `[A-Za-z0-9_-]`, starting with an ASCII letter — see module docs for the rationale.
pub(crate) fn validate_variable_name(name: &str) -> Result<(), VariableNameError> {
    if name.is_empty() {
        Err(VariableNameError::Empty)
    } else if name.len() > VARIABLE_NAME_MAX_LENGTH {
        Err(VariableNameError::TooLong {
            length: name.len(),
            max: VARIABLE_NAME_MAX_LENGTH,
        })
    } else if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        Err(VariableNameError::InvalidStart)
    } else if let Some(invalid) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
    {
        Err(VariableNameError::InvalidCharacter(invalid))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    #[rstest]
    #[case::simple("bin")]
    #[case::camel_case("agentConfigMap")]
    #[case::snake_case("health_imagePullPolicy")]
    #[case::kebab_case("k8s-agents-operator")]
    #[case::contains_digits("agent2Config9")]
    #[case::single_letter("a")]
    #[case::max_length(&"a".repeat(VARIABLE_NAME_MAX_LENGTH))]
    fn valid_names_are_accepted(#[case] name: &str) {
        assert_matches!(validate_variable_name(name), Ok(()));
    }

    #[rstest]
    #[case::empty("", VariableNameError::Empty)]
    #[case::starts_with_digit("1abc", VariableNameError::InvalidStart)]
    #[case::starts_with_dash("-abc", VariableNameError::InvalidStart)]
    #[case::starts_with_underscore("_abc", VariableNameError::InvalidStart)]
    #[case::contains_dot("foo.bar", VariableNameError::InvalidCharacter('.'))]
    #[case::contains_colon("foo:bar", VariableNameError::InvalidCharacter(':'))]
    #[case::contains_space("foo bar", VariableNameError::InvalidCharacter(' '))]
    #[case::contains_slash("foo/bar", VariableNameError::InvalidCharacter('/'))]
    #[case::contains_dollar("foo$bar", VariableNameError::InvalidCharacter('$'))]
    fn invalid_names_are_rejected(#[case] name: &str, #[case] expected: VariableNameError) {
        assert_eq!(validate_variable_name(name), Err(expected));
    }

    #[test]
    fn too_long_name_is_rejected() {
        let name = "a".repeat(VARIABLE_NAME_MAX_LENGTH + 1);
        assert_eq!(
            validate_variable_name(&name),
            Err(VariableNameError::TooLong {
                length: VARIABLE_NAME_MAX_LENGTH + 1,
                max: VARIABLE_NAME_MAX_LENGTH,
            })
        );
    }
}
