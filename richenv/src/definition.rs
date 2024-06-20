use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Deserializer};
use thiserror::Error;

use crate::definition::EnvVarDefinitionError::{
    InvalidKeyFormat, InvalidKeyOrValueFormat, UnexpectedErrorParsingEnvRegex,
};
use crate::envvar::env_var_key_regex;

// Regex to validate prefixed environment variables keys.
// Optional Env Var format:
// uppercase letters, digits, and the '_' (underscore) from the characters defined in
// Portable Character Set and do not begin with a digit
// https://pubs.opengroup.org/onlinepubs/000095399/basedefs/xbd_chap08.html
//
// Followed by a `*`
const PREFIXED_ENV_VAR_KEY_REGEX: &str = r"^([a-zA-Z_][a-zA-Z0-9_]*)?\*$";

// build regex to validate env var keys just once
fn prefixed_env_var_key_regex() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(PREFIXED_ENV_VAR_KEY_REGEX).unwrap())
}

#[derive(Error, Debug, Clone)]
pub enum EnvVarDefinitionError {
    #[error("key `{0}` is invalid")]
    InvalidKeyFormat(String),
    #[error("key `{0}` or value `{0}` are not correct")]
    InvalidKeyOrValueFormat(String, String),
    #[error("unexpected error while parsing env var `{0}`")]
    UnexpectedErrorParsingEnvRegex(String),
}

// Types to increase readability and help avoiding mistakes
// Tuple (LiteralTo, LiteralFrom) better than (String, String)
type PrefixableFrom = DefinitionKeyOrFrom;
type PrefixableTo = DefinitionKeyOrFrom;
type LiteralValue = String;
type LiteralTo = String;
type MappingFrom = String;
type MappingTo = String;

/// EnvVarsDefinition is the structure that will contain all the Rules for env forwarding.
/// Rules:
///  * Literal: An Env var created with a fixed vale
///  * Mapping: An Env var mapped from another env var
///  * Prefixed: All env vars starting with Prefix A (might be empty) will be converted to
///              env var with prefix B (might be empty)
///
/// The structure has been designed to validate the rules creation through deserialization.
/// Origin configuration example:
///   env:
///     # Literal
///     SOME_VARNAME:
///       value: 12334
///
///     # Mapping
///     NEW_VAR_NAME:
///       from: ORIGIN_VAR_NAME
///
///     # Prefixed Origin Prefix: VAR_ / Dest Prefix: PREFIXED_VAR_
///     PREFIXED_VAR_*:
///       from: VAR_*
///
///     # Prefixed (forwarding, no substitution)
///     PREFIXED_VAR_*:
///       from: PREFIXED_VAR_*
///
///     # Prefixed (strip an existing prefix)
///     *:
///       from: RANDOM_PREFIX_*
///
///     # Prefixed (add a prefix to everything)
///     SOME_PREFIX_*:
///       from: *
///
///     # Forward everything
///     *:
///       from: *
#[derive(Debug)]
pub struct EnvVarsDefinition {
    rules: EnvDefRules,
}

impl EnvVarsDefinition {
    fn new(rules: EnvDefRules) -> Self {
        Self { rules }
    }
}

// EnvDefRules is a vector as we might want to order the rules
// to be applied. We could change it to HashMap if not really needed
type EnvDefRules = Vec<EnvDefRule>;

#[derive(Debug, PartialEq, Clone)]
enum EnvDefRule {
    Literal(LiteralValue, LiteralTo),
    Mapping(MappingFrom, MappingTo),
    Prefixed(PrefixableFrom, PrefixableTo),
}

/// DefinitionKeyVal contains the value of a definition key or value
/// which follow the same format rules
/// Both can be a valid env var key type or a valid env var key type + wildcard
/// We will only allow constructing it through try_from to ensure we always have a
/// valid struct and that we only execute validation regex once.
/// It'll contain the prefix, so we don't need to execute the regex more than once
#[derive(Debug, Eq, Clone)]
struct DefinitionKeyOrFrom {
    value: String,
    prefix: Option<String>,
}

// Necessary to DefinitionKeyAndFrom as key in a HashMap
impl Hash for DefinitionKeyOrFrom {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

// Necessary to DefinitionKeyAndFrom as key in a HashMap
impl PartialEq for DefinitionKeyOrFrom {
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(other.value.as_str())
    }
}

impl DefinitionKeyOrFrom {
    pub fn prefix(&self) -> &Option<String> {
        &self.prefix
    }

    pub fn is_prefixed(&self) -> bool {
        self.prefix.is_some()
    }

    pub fn value(&self) -> &str {
        self.value.as_str()
    }
}

/// Create DefinitionKeyOrFrom from a string. It will detect the prefix if exists,
/// and ensure that it is a valid env var key. (Having prefix means that the string
/// ends with a wildcard `*`)
///
/// Valid values:
/// PREFIX_1_*
/// *
/// NO_PREFIX_1
///
/// Invalid values:
/// TWO_WILDCARDS_*_*
/// WILDCARD_IN_*_THE_MIDDLE
/// 5_NOT_VALID_ENV_VAR
/// 5_PREFIX_*
impl TryFrom<String> for DefinitionKeyOrFrom {
    type Error = EnvVarDefinitionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(InvalidKeyFormat(value));
        }
        // Detect and store prefix if present, so we don't execute regex twice
        match prefixed_env_var_key_regex().captures(value.as_str()) {
            Some(captured) => {
                if let Some(prefix) = captured.get(1) {
                    Ok(Self {
                        prefix: Some(String::from(prefix.as_str())),
                        value,
                    })
                } else {
                    Err(UnexpectedErrorParsingEnvRegex(value))
                }
            }
            // If the key is not prefixed, ensure that it is a valid Env var key
            None => {
                if !env_var_key_regex().is_match(value.as_str()) {
                    Err(InvalidKeyFormat(value))
                } else {
                    Ok(Self {
                        prefix: None,
                        value,
                    })
                }
            }
        }
    }
}

// Deserialize an env var key or from a string. It detects if the env var
// has a prefix and if this is a valid Env var key
impl<'de> Deserialize<'de> for DefinitionKeyOrFrom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let strval = String::deserialize(deserializer)?;
        match DefinitionKeyOrFrom::try_from(strval) {
            Err(e) => Err(Error::custom(e)),
            Ok(d) => Ok(d),
        }
    }
}

/// Deserialize Env vars definition yaml to EnvVarsDefinition structure
/// Rules are composed of two elements `from` & `to` and they come from
/// the yaml map key and value, so we deserialize the whole yaml to a map
/// and parse each element to create each rule.
impl<'de> Deserialize<'de> for EnvVarsDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        // intermediate serialization type to read `from` or `value` item from yaml
        #[derive(Deserialize, Debug, Hash, PartialEq, Eq)]
        #[serde(rename_all = "snake_case")]
        #[serde(untagged)]
        enum IntermediateEnvVarType {
            From { from: DefinitionKeyOrFrom },
            Value { value: String },
        }

        // Intermediate serialization type to deserialize the whole structure.
        // Keys and values individually are validated through deserialization
        // (DefinitionKeyOrFrom,IntermediateEnvVarType)
        // After validating the keys and values, the rule creation needs
        // to be validated
        #[derive(Debug, Deserialize)]
        struct IntermediateEnvVarsDefinition {
            env: HashMap<DefinitionKeyOrFrom, IntermediateEnvVarType>,
        }

        // Dest rules to be returned
        let mut rules: EnvDefRules = EnvDefRules::default();

        // deserialize to hashmap and iterate over each element to create a rule
        let intermediate_val = IntermediateEnvVarsDefinition::deserialize(deserializer)?;
        for (dest_var, origin_var) in intermediate_val.env.into_iter() {
            match origin_var {
                IntermediateEnvVarType::Value { value } => {
                    if dest_var.is_prefixed() {
                        return Err(Error::custom(InvalidKeyFormat(dest_var.value)));
                    }
                    rules.push(EnvDefRule::Literal(value, LiteralTo::from(dest_var.value)));
                }
                IntermediateEnvVarType::From { from } => {
                    // key and value should be both prefixed or not prefixed
                    if dest_var.is_prefixed() ^ from.is_prefixed() {
                        return Err(Error::custom(InvalidKeyOrValueFormat(
                            dest_var.value,
                            from.value,
                        )));
                    }

                    if dest_var.is_prefixed() {
                        rules.push(EnvDefRule::Prefixed(from, dest_var));
                    } else {
                        rules.push(EnvDefRule::Mapping(from.value, dest_var.value));
                    }
                }
            }
        }

        Ok(EnvVarsDefinition::new(rules))
    }
}

#[cfg(test)]
mod test {
    use crate::definition::{
        DefinitionKeyOrFrom, EnvDefRule, EnvDefRules, EnvVarDefinitionError, EnvVarsDefinition,
        LiteralTo, LiteralValue, MappingFrom, MappingTo, PrefixableFrom, PrefixableTo,
    };
    use std::fmt::{Display, Formatter};

    impl Display for EnvDefRule {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            let key = match self {
                EnvDefRule::Literal(_, to) => to,
                EnvDefRule::Mapping(_, to) => to,
                EnvDefRule::Prefixed(_, to) => to.value(),
            };
            write!(f, "{}", key)
        }
    }

    #[test]
    fn test_invalid_definitions() {
        struct TestCase {
            _name: &'static str,
            content: &'static str,
            expected: String,
        }
        impl TestCase {
            fn run(self) {
                let err = serde_yaml::from_str::<EnvVarsDefinition>(self.content).unwrap_err();
                // TODO: how can I assert on the origin error? assert_matches cannot be used
                // as the error captured is serde_yaml::Error
                assert_eq!(err.to_string(), self.expected);
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "from cannot be empty",
                content: r#"
                env:
                  VAR_NAME_*:
                    from: ""
                  ANOTHER_VAR:
                    value: "lala"
                  PREFIXED_VAR_*:
                    from: VAR_*
                "#,
                expected: String::from("env: data did not match any variant of untagged enum IntermediateEnvVarType at line 3 column 19"),
            },
            TestCase {
                _name: "from not prefixed and key prefixed is not allowed",
                content: r#"
                env:
                  PREFIXED_*:
                    from: "NOT_PREFIXED"
                  ANOTHER_VAR:
                    value: "lala"
                  PREFIXED_VAR_*:
                    from: VAR_*
                "#,
                expected: EnvVarDefinitionError::InvalidKeyOrValueFormat(String::from("PREFIXED_*"),String::from("NOT_PREFIXED")).to_string(),
            },
            TestCase {
                _name: "from prefixed and key not prefixed is not allowed",
                content: r#"
                env:
                  NOT_PREFIXED:
                    from: "PREFIXED_*"
                  ANOTHER_VAR:
                    value: "lala"
                  PREFIXED_VAR_*:
                    from: VAR_*
                "#,
                expected: EnvVarDefinitionError::InvalidKeyOrValueFormat(String::from("NOT_PREFIXED"),String::from("PREFIXED_*")).to_string(),
            },
            TestCase {
                _name: "value cannot be prefixed",
                content: r#"
                env:
                  PREFIXED_CHANGED_*:
                    from: "PREFIXED_*"
                  ANOTHER_VAR_*:
                    value: "lala"
                  PREFIXED_VAR_*:
                    from: VAR_*
                "#,
                expected: EnvVarDefinitionError::InvalidKeyFormat(String::from("ANOTHER_VAR_*")).to_string(),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_valid_definitions() {
        struct TestCase {
            _name: &'static str,
            content: &'static str,
            expected: EnvDefRules,
        }
        impl TestCase {
            fn run(self) {
                let mut env_vars_definition =
                    serde_yaml::from_str::<EnvVarsDefinition>(self.content).unwrap();

                //sort the rules for comparison
                env_vars_definition
                    .rules
                    .sort_by(|a, b| a.to_string().cmp(&b.to_string()));

                let mut expected = self.expected.clone();
                expected.sort_by(|a, b| a.to_string().cmp(&b.to_string()));

                assert_eq!(env_vars_definition.rules, expected);
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "from cannot be empty",
                content: r#"
                env:
                  VAR_NAME_*:
                    from: "VAR2_NAME_*"
                  ANOTHER_VAR:
                    value: "lala"
                  ANOTHER_LITERAL:
                    value: "some value with spaces"
                  PREFIXED_VAR_ONE:
                    from: VAR_ONE
                  ANOTHER_PREFIX_*:
                    from: SOME_OTHER_PREFIX_*
                "#,
                expected: vec![
                    EnvDefRule::Prefixed(
                        PrefixableFrom::try_from(String::from("VAR2_NAME_*")).unwrap(),
                        PrefixableTo::try_from(String::from("VAR_NAME_*")).unwrap(),
                    ),
                    EnvDefRule::Literal(LiteralValue::from("lala"), LiteralTo::from("ANOTHER_VAR")),
                    EnvDefRule::Mapping(
                        MappingFrom::from("VAR_ONE"),
                        MappingTo::from("PREFIXED_VAR_ONE"),
                    ),
                    EnvDefRule::Literal(
                        LiteralValue::from("some value with spaces"),
                        LiteralTo::from("ANOTHER_LITERAL"),
                    ),
                    EnvDefRule::Prefixed(
                        PrefixableFrom::try_from(String::from("SOME_OTHER_PREFIX_*")).unwrap(),
                        PrefixableTo::try_from(String::from("ANOTHER_PREFIX_*")).unwrap(),
                    ),
                ],
            },
            TestCase {
                _name: "from cannot be empty",
                content: r#"
                env: {}
                "#,
                expected: Vec::default(),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_definition_key_or_from_from_string() {
        struct TestCase {
            name: &'static str,
            input: String,
            expected: DefinitionKeyOrFrom,
        }
        impl TestCase {
            fn run(self) {
                assert_eq!(
                    DefinitionKeyOrFrom::try_from(self.input).unwrap(),
                    self.expected
                )
            }
        }
        let test_cases = vec![
            TestCase {
                name: "no prefix",
                input: String::from("NO_PREFIX"),
                expected: DefinitionKeyOrFrom {
                    value: String::from("NO_PREFIX"),
                    prefix: None,
                },
            },
            TestCase {
                name: "prefix",
                input: String::from("PREFIX_*"),
                expected: DefinitionKeyOrFrom {
                    value: String::from("PREFIX_*"),
                    prefix: Some(String::from("PREFIX_")),
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
