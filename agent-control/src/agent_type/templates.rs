use super::definition::Variables;
use super::error::AgentTypeError;
use super::variable::definition::VariableDefinition;
use super::variable::kind::Kind;
use regex::Regex;
use std::sync::OnceLock;

/// Regex that extracts the template values from a string.
///
/// Example:
///
/// ```
/// use regex::Regex;
///
/// const TEMPLATE_RE: &str = r"\$\{(nr-[a-z]+:[a-zA-Z0-9\.\-_/]+)\}";
/// let re = Regex::new(TEMPLATE_RE).unwrap();
/// let content = "Hello ${nr-var:name.value}!";
///
/// let result = re.find_iter(content).map(|i| i.as_str()).collect::<Vec<_>>();
///
/// assert_eq!(result, vec!["${nr-var:name.value}"]);
const TEMPLATE_RE: &str = r"\$\{(nr-[a-z]+:[a-zA-Z0-9\.\-_/]+)\}";
const TEMPLATE_BEGIN: &str = "${";
const TEMPLATE_END: char = '}';
pub const TEMPLATE_KEY_SEPARATOR: &str = ".";

pub trait Templateable {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

fn template_re() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(TEMPLATE_RE).unwrap())
}

fn only_template_var_re() -> &'static Regex {
    static ONLY_RE_ONCE: OnceLock<Regex> = OnceLock::new();
    ONLY_RE_ONCE.get_or_init(|| Regex::new(format!("^{TEMPLATE_RE}$").as_str()).unwrap())
}

/// Returns a string slice with the template's begin and end trimmed.
fn template_trim(s: &str) -> &str {
    s.trim_start_matches(TEMPLATE_BEGIN)
        .trim_end_matches(TEMPLATE_END)
}

/// Returns a variable reference from the provided set if it exists, it returns an error otherwise.
fn normalized_var<'a>(
    name: &str,
    variables: &'a Variables,
) -> Result<&'a VariableDefinition, AgentTypeError> {
    variables
        .get(name)
        .ok_or(AgentTypeError::MissingTemplateKey(name.to_string()))
}

/// Returns a string with the variable replaced with the corresponding value .
fn replace(
    variable: &str,
    s: &str,
    normalized_var: &VariableDefinition,
) -> Result<String, AgentTypeError> {
    let value = normalized_var
        .get_template_value()
        .ok_or(AgentTypeError::MissingTemplateKey(variable.to_string()))?
        .to_string();

    Ok(s.replace(variable, value.as_str()))
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, variables: &Variables) -> Result<String, AgentTypeError> {
        template_string(self, variables)
    }
}

fn template_string(s: String, variables: &Variables) -> Result<String, AgentTypeError> {
    let re = template_re();
    re.find_iter(&s)
        .try_fold(s.clone(), |r, variable_to_substitute| {
            let var_name = template_trim(variable_to_substitute.as_str());
            let normalized_var = normalized_var(var_name, variables)?;
            replace(variable_to_substitute.as_str(), &r, normalized_var)
        })
}

impl Templateable for serde_yaml::Value {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_value = match self {
            serde_yaml::Value::Mapping(m) => {
                serde_yaml::Value::Mapping(m.template_with(variables)?)
            }
            serde_yaml::Value::Sequence(seq) => {
                serde_yaml::Value::Sequence(seq.template_with(variables)?)
            }
            serde_yaml::Value::String(st) => template_yaml_value_string(st, variables)?,
            _ => self,
        };

        Ok(templated_value)
    }
}

impl Templateable for serde_yaml::Mapping {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect()
    }
}

impl Templateable for serde_yaml::Sequence {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|v| v.template_with(variables))
            .collect()
    }
}

/// Templates yaml strings as [serde_yaml::Value].
/// When all the string content is a variable template, the corresponding variable type is checked
/// and the value is handled as needed. Otherwise, it is templated as a regular string. Example:
///
/// ```yaml
/// key1: ${var} # The var type is checked and the expanded value might not be a string.
/// # The examples below are always templated as string, regardless of the variable type.
/// key2: this-${var}
/// key3: ${var}${var}
/// ```
fn template_yaml_value_string(
    s: String,
    variables: &Variables,
) -> Result<serde_yaml::Value, AgentTypeError> {
    // When there is more content than a variable template, template as a regular string.
    if !only_template_var_re().is_match(s.as_str()) {
        let templated = template_string(s, variables)?;
        return Ok(serde_yaml::Value::String(templated));
    }
    // Otherwise, template according to the variable type.
    let var_name = template_trim(s.as_str());
    let var_spec = normalized_var(var_name, variables)?;
    let var_value = var_spec
        .get_template_value()
        .ok_or(AgentTypeError::MissingValue(var_name.to_string()))?;

    match var_spec.kind() {
        Kind::Yaml(_) => var_value
            .to_yaml_value()
            .ok_or(AgentTypeError::UnexpectedValueForKey(
                var_name.to_string(),
                var_value.to_string(),
            )),
        Kind::Bool(_) | Kind::Number(_) => {
            serde_yaml::from_str(var_value.to_string().as_str()).map_err(AgentTypeError::SerdeYaml)
        }
        _ => Ok(serde_yaml::Value::String(var_value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::variable::kind_value::KindValue;
    use assert_matches::assert_matches;
    use serde_yaml::Number;

    #[test]
    fn test_template_string() {
        let variables = Variables::from([
            (
                "nr-var:name".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("Alice ${UNTOUCHED}".to_string()),
                ),
            ),
            (
                "nr-var:age".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(30))),
            ),
        ]);

        let input =
            "Hello ${nr-var:name}! You are ${nr-var:age} years old. ${UNTOUCHED}".to_string();
        let expected_output =
            "Hello Alice ${UNTOUCHED}! You are 30 years old. ${UNTOUCHED}".to_string();
        let actual_output = template_string(input, &variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_mapping() {
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING ${UNTOUCHED}".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
        ]);
        let input: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "${nr-var:change.me.string}"
        a_boolean: "${nr-var:change.me.bool}"
        a_number: "${nr-var:change.me.number}"
        ${nr-var:change.me.string}: "Do not scape me"
        ${nr-var:change.me.bool}: "Do not scape me"
        ${nr-var:change.me.number}: "Do not scape me"
        test: ${UNTOUCHED}
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "CHANGED-STRING ${UNTOUCHED}"
        a_boolean: true
        a_number: 42
        ${nr-var:change.me.string}: "Do not scape me"
        ${nr-var:change.me.bool}: "Do not scape me"
        ${nr-var:change.me.number}: "Do not scape me"
        test: ${UNTOUCHED}
        "#,
        )
        .unwrap();

        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_sequence() {
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING ${UNTOUCHED}".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
        ]);
        let input: serde_yaml::Sequence = serde_yaml::from_str(
            r#"
        - ${nr-var:change.me.string}
        - ${nr-var:change.me.bool}
        - ${nr-var:change.me.number}
        - ${UNTOUCHED}
        - Do not scape me
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Sequence = serde_yaml::from_str(
            r#"
        - CHANGED-STRING ${UNTOUCHED}
        - true
        - 42
        - ${UNTOUCHED}
        - Do not scape me
        "#,
        )
        .unwrap();

        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_yaml() {
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING ${UNTOUCHED}".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
            (
                "nr-var:change.me.yaml".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                        [("key".into(), "value".into())],
                    ))),
                ),
            ),
            (
                // Expansion inside variable's values is not supported.
                "nr-var:yaml.with.var.placeholder".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                        [(
                            "this.will.not.be.expanded".into(),
                            "${nr-var:change.me.string}".into(),
                        )],
                    ))),
                ),
            ),
        ]);
        let input: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: ${nr-var:change.me.string}
            a_boolean: ${nr-var:change.me.bool}
            a_number: ${nr-var:change.me.number}
        a_sequence:
            - ${nr-var:change.me.string}
            - ${nr-var:change.me.bool}
            - ${nr-var:change.me.number}
        a_nested_object:
            with_nested_sequence:
                - a_string: ${nr-var:change.me.string}
                - a_boolean: ${nr-var:change.me.bool}
                - a_number: ${nr-var:change.me.number}
                - a_yaml: ${nr-var:change.me.yaml}
        a_string: ${nr-var:change.me.string}
        a_boolean: ${nr-var:change.me.bool}
        a_number: ${nr-var:change.me.number}
        a_yaml: ${nr-var:change.me.yaml}
        another_yaml: ${nr-var:yaml.with.var.placeholder} # A variable inside another variable value is not expanded
        string_key: "here, the value ${nr-var:change.me.yaml} is encoded as string because it is not alone"
        last_one: ${UNTOUCHED}
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: "CHANGED-STRING ${UNTOUCHED}"
            a_boolean: true
            a_number: 42
        a_sequence:
            - "CHANGED-STRING ${UNTOUCHED}"
            - true
            - 42
        a_nested_object:
            with_nested_sequence:
                - a_string: "CHANGED-STRING ${UNTOUCHED}"
                - a_boolean: true
                - a_number: 42
                - a_yaml:
                    key:
                      value
        a_string: "CHANGED-STRING ${UNTOUCHED}"
        a_boolean: true
        a_number: 42
        a_yaml:
          key: value
        another_yaml:
          "this.will.not.be.expanded": "${nr-var:change.me.string}" # A variable inside another other variable value is not expanded
        string_key: "here, the value key: value\n is encoded as string because it is not alone"
        last_one: ${UNTOUCHED}
        "#, // FIXME? Note line above, the "key: value\n" part was replaced!!
        )
        .unwrap();

        let actual_output: serde_yaml::Value = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_fail_template_yaml_value_string() {
        struct TestCase {
            name: &'static str,
            variables: Variables,
            input: &'static str,
            assert_fn: fn(AgentTypeError),
        }
        impl TestCase {
            fn run(self) {
                let actual_err =
                    template_yaml_value_string(self.input.to_string(), &self.variables)
                        .expect_err(format!("error is expected, case: {}", self.name).as_str());
                (self.assert_fn)(actual_err);
            }
        }
        let test_cases = vec![
            TestCase {
                name: "trying to replace a variable that is not defined",
                variables: Variables::new(),
                input: "${nr-var:not-defined}",
                assert_fn: |err| assert_matches!(err, AgentTypeError::MissingTemplateKey(_)),
            },
            TestCase {
                name: "missing required value key",
                variables: Variables::from([(
                    "nr-var:yaml".to_string(),
                    KindValue::<serde_yaml::Value>::new(true, None, None).into(),
                )]),
                input: "${nr-var:yaml}",
                assert_fn: |err| assert_matches!(err, AgentTypeError::MissingValue(_)),
            },
            TestCase {
                name: "missing non-required key",
                variables: Variables::from([(
                    "nr-var:yaml".to_string(),
                    KindValue::<serde_yaml::Value>::new(false, None, None).into(),
                )]),
                input: "${nr-var:yaml}",
                assert_fn: |err| assert_matches!(err, AgentTypeError::MissingValue(_)),
            },
        ];
        for test_case in test_cases {
            test_case.run();
        }
    }
    #[test]
    fn test_template_yaml_value_string() {
        struct TestCase {
            name: &'static str,
            variables: Variables,
            expectations: Vec<(&'static str, serde_yaml::Value)>,
        }
        impl TestCase {
            fn run(self) {
                for (input, expected_output) in self.expectations {
                    assert_eq!(
                        expected_output,
                        template_yaml_value_string(input.to_string(), &self.variables)
                            .unwrap_or_else(|_| panic!("failed templating, case: {}", self.name)),
                        "failed, case: {}",
                        self.name
                    );
                }
            }
        }
        let test_cases = vec![
            TestCase {
                name: "simple string",
                variables: Variables::from([(
                    "nr-var:simple.string.var".to_string(),
                    VariableDefinition::new(
                        String::default(),
                        true,
                        None,
                        Some("Value".to_string()),
                    ),
                )]),
                expectations: vec![
                    (
                        "${nr-var:simple.string.var}",
                        serde_yaml::Value::String("Value".into()),
                    ),
                    (
                        "var=${nr-var:simple.string.var}",
                        serde_yaml::Value::String("var=Value".into()),
                    ),
                    (
                        "${nr-var:simple.string.var}${nr-var:simple.string.var}",
                        serde_yaml::Value::String("ValueValue".into()),
                    ),
                ],
            },
            TestCase {
                name: "string with yaml",
                variables: Variables::from([(
                    "nr-var:string.with.yaml.var".to_string(),
                    VariableDefinition::new(
                        String::default(),
                        true,
                        None,
                        Some("[Value]".to_string()),
                    ),
                )]),
                expectations: vec![(
                    "${nr-var:string.with.yaml.var}",
                    serde_yaml::Value::String("[Value]".into()),
                )],
            },
            TestCase {
                name: "bool",
                variables: Variables::from([(
                    "nr-var:bool.var".to_string(),
                    VariableDefinition::new(String::default(), true, None, Some(true)),
                )]),
                expectations: vec![
                    ("${nr-var:bool.var}", serde_yaml::Value::Bool(true)),
                    (
                        "${nr-var:bool.var}${nr-var:bool.var}",
                        serde_yaml::Value::String("truetrue".into()),
                    ),
                ],
            },
            TestCase {
                name: "number",
                variables: Variables::from([(
                    "nr-var:number.var".to_string(),
                    VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
                )]),
                expectations: vec![(
                    "${nr-var:number.var}",
                    serde_yaml::Value::Number(serde_yaml::Number::from(42i32)),
                )],
            },
            TestCase {
                name: "number, bool, and string",
                variables: Variables::from([
                    (
                        "nr-var:number.var".to_string(),
                        VariableDefinition::new(
                            String::default(),
                            true,
                            None,
                            Some(Number::from(42)),
                        ),
                    ),
                    (
                        "nr-var:bool.var".to_string(),
                        VariableDefinition::new(String::default(), true, None, Some(true)),
                    ),
                    (
                        "nr-var:simple.string.var".to_string(),
                        VariableDefinition::new(
                            String::default(),
                            true,
                            None,
                            Some("Value".to_string()),
                        ),
                    ),
                ]),
                expectations: vec![
                    (
                        r#"${nr-var:bool.var}${nr-var:number.var}"#,
                        serde_yaml::Value::String("true42".into()),
                    ),
                    (
                        r#"the ${nr-var:number.var} ${nr-var:simple.string.var} is ${nr-var:bool.var}"#,
                        serde_yaml::Value::String("the 42 Value is true".into()),
                    ),
                ],
            },
            TestCase {
                name: "yaml",
                variables: Variables::from([(
                    "nr-var:yaml.var".to_string(),
                    VariableDefinition::new(
                        String::default(),
                        true,
                        None,
                        Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                            [("key".into(), "value".into())],
                        ))),
                    ),
                )]),
                expectations: vec![
                    (
                        "${nr-var:yaml.var}",
                        serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
                            "key".into(),
                            "value".into(),
                        )])),
                    ),
                    (
                        "x: ${nr-var:yaml.var}",
                        serde_yaml::Value::String("x: key: value\n".into()), // FIXME? Consder if this is ok.
                    ),
                ],
            },
            TestCase {
                name: "yaml from default value",
                variables: Variables::from([(
                    "nr-var:yaml.var".to_string(),
                    VariableDefinition::new(
                        String::default(),
                        false,
                        Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                            [("key".into(), "value".into())],
                        ))),
                        None,
                    ),
                )]),
                expectations: vec![(
                    "${nr-var:yaml.var}",
                    serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
                        "key".into(),
                        "value".into(),
                    )])),
                )],
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_normalized_var() {
        let variables = Variables::from([(
            "nr-var:var.name".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("Value".to_string())),
        )]);

        assert_matches!(
            normalized_var("nr-var:var.name", &variables)
                .unwrap()
                .kind(),
            Kind::String(_)
        );
        let key = assert_matches!(
            normalized_var("does.not.exists", &variables).unwrap_err(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("does.not.exists".to_string(), key);
    }

    #[test]
    fn test_replace() {
        let value_var =
            VariableDefinition::new(String::default(), true, None, Some("Value".to_string()));
        let default_var =
            VariableDefinition::new(String::default(), true, Some("Default".to_string()), None);

        let neither_value_nor_default =
            VariableDefinition::new(String::default(), true, None::<String>, None::<String>);

        assert_eq!(
            "Value-${nr-var:other}".to_string(),
            replace("${nr-var:any}", "${nr-var:any}-${nr-var:other}", &value_var).unwrap()
        );
        assert_eq!(
            "Default-${nr-var:other}".to_string(),
            replace(
                "${nr-var:any}",
                "${nr-var:any}-${nr-var:other}",
                &default_var
            )
            .unwrap()
        );
        let key = assert_matches!(
            replace("${nr-var:any}", "${nr-var:any}-x", &neither_value_nor_default).unwrap_err(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("${nr-var:any}".to_string(), key);
    }
}
