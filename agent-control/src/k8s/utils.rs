use super::Error;
use crate::k8s::error::K8sError;
use k8s_openapi::{
    Metadata, NamespaceResourceScope, Resource, apimachinery::pkg::util::intstr::IntOrString,
};
use kube::api::{DynamicObject, ObjectMeta, TypeMeta};
use serde_yaml::{Mapping, Value};

/// This is a helper to have the number of pods or percentages for update strategies.
///
/// You can get this enum from `IntOrString` but it could return an error if it is not parsable.
/// `IntOrString` is used on other parts of the API like pod ports (80 or "http") so casting it
/// is not safe from errors if incorrectly used.
///
/// ```
/// use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
/// use newrelic_agent_control::k8s::utils::IntOrPercentage;
///
/// let int_or_string_int = IntOrString::Int(1);
/// let int_or_string_string = IntOrString::String("1".into());
/// assert_eq!(
///   IntOrPercentage::try_from(int_or_string_int).unwrap(),
///   IntOrPercentage::try_from(int_or_string_string).unwrap(),
/// );
///
/// let percent_string = "50%";
/// let percent_literal = 50.0/100.0;
/// let IntOrPercentage::Percentage(percent_parsed) = IntOrPercentage::try_from(percent_string).unwrap() else { todo!() };
/// assert_eq!(percent_literal, percent_parsed);
///
/// let int_string = "5";
/// let int_literal = 5;
/// let IntOrPercentage::Int(int_parsed) = IntOrPercentage::try_from(int_string).unwrap() else { todo!() };
/// assert_eq!(int_literal, int_parsed);
/// ```
#[derive(Debug, PartialEq)]
pub enum IntOrPercentage {
    Int(i32),
    Percentage(f32),
}

impl TryFrom<IntOrString> for IntOrPercentage {
    type Error = std::num::ParseIntError;

    fn try_from(value: IntOrString) -> Result<Self, Self::Error> {
        match value {
            IntOrString::Int(i) => Ok(IntOrPercentage::Int(i)),
            IntOrString::String(s) => IntOrPercentage::try_from(s),
        }
    }
}

impl TryFrom<&str> for IntOrPercentage {
    type Error = std::num::ParseIntError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        IntOrPercentage::try_from(value.to_string())
    }
}

impl TryFrom<String> for IntOrPercentage {
    type Error = std::num::ParseIntError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if let Some(percent) = value.strip_suffix('%') {
            let parsed = percent.parse::<i32>()?;
            return Ok(IntOrPercentage::Percentage(parsed as f32 / 100.0));
        }

        let parsed = value.parse::<i32>()?;
        Ok(IntOrPercentage::Int(parsed))
    }
}

impl IntOrPercentage {
    /// Returns a scaled value from an IntOrPercentage type. If the IntOrPercentage is a percentage
    /// it's treated as a percentage and scaled appropriately in accordance to the total, if it's
    /// an int value it's treated as a simple value.
    ///
    /// This function mimics a missing function from apimachinery that rust does not have but
    /// go-client has:
    /// https://pkg.go.dev/k8s.io/apimachinery/pkg/util/intstr#GetScaledValueFromIntOrPercent
    ///
    /// ```
    /// use newrelic_agent_control::k8s::utils::IntOrPercentage;
    ///
    /// let int = IntOrPercentage::try_from("5").unwrap();
    /// let percent = IntOrPercentage::try_from("33%").unwrap();
    /// let total = 20;
    ///
    /// assert_eq!(int.scaled_value(total, false), 5);
    /// assert_eq!(percent.scaled_value(total, false), 6);
    /// assert_eq!(percent.scaled_value(total, true), 7);
    /// ```
    pub fn scaled_value(&self, total: i32, round_up: bool) -> i32 {
        match self {
            IntOrPercentage::Int(i) => *i,
            IntOrPercentage::Percentage(percent) => {
                if round_up {
                    (total as f32 * *percent).ceil() as i32
                } else {
                    (total as f32 * *percent).floor() as i32
                }
            }
        }
    }
}

impl std::fmt::Display for IntOrPercentage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntOrPercentage::Int(i) => write!(f, "{i}"),
            IntOrPercentage::Percentage(float) => {
                let percent = (*float * 100.0) as i32;
                write!(f, "{percent}%")
            }
        }
    }
}

/// Return the value of `.metadata.name` of the object that is passed.
pub fn get_metadata_name<K>(obj: &K) -> Result<String, Error>
where
    K: Resource<Scope = NamespaceResourceScope> + Metadata<Ty = ObjectMeta>,
{
    let metadata = obj.metadata();

    metadata
        .name
        .clone()
        .ok_or_else(|| Error::MissingName(K::KIND.to_string()))
}

pub fn display_type(type_meta: &TypeMeta) -> String {
    format!(
        "apiVersion: {} kind: {}",
        type_meta.api_version, type_meta.kind
    )
}

pub fn get_name(obj: &DynamicObject) -> Result<String, K8sError> {
    obj.metadata
        .clone()
        .name
        .ok_or(K8sError::MissingResourceName)
}

pub fn get_namespace(obj: &DynamicObject) -> Result<String, K8sError> {
    obj.metadata
        .clone()
        .namespace
        .ok_or(K8sError::MissingResourceNamespace)
}

pub fn get_type_meta(obj: &DynamicObject) -> Result<TypeMeta, K8sError> {
    obj.types.clone().ok_or(K8sError::MissingResourceKind)
}

pub fn get_target_namespace(obj: &DynamicObject) -> Option<String> {
    obj.data.get("spec").and_then(|spec| {
        spec.get("targetNamespace")
            // Passing through the str is needed to avoid quotes
            .map(|v| v.as_str().unwrap_or_default().to_string())
    })
}

/// This function recursively traverses the mapping structure and removes any key-value
/// pairs where the value is `Value::Null`.
pub fn retain_not_null(mapping: &mut Mapping) {
    mapping.retain(|_, value| match value {
        Value::Null => false,
        Value::Mapping(inner_mapping) => {
            retain_not_null(inner_mapping);
            true
        }
        _ => true,
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};

    #[test]
    fn test_retain_not_null() {
        let mut input = serde_yaml::from_str::<Mapping>(
            r#"
        should_retain_string: some
        should_retain_bool: true
        should_retain_slice: [1,2,3]
        should_retain_number: 0
        should_retain_empty_mapping: {}
        should_removed: Null
        nested:
          should_retain: some
          should_removed: Null
        "#,
        )
        .unwrap();
        let expected = serde_yaml::from_str::<Mapping>(
            r#"
        should_retain_string: some
        should_retain_bool: true
        should_retain_slice: [1,2,3]
        should_retain_number: 0
        should_retain_empty_mapping: {}
        nested:
          should_retain: some
        "#,
        )
        .unwrap();

        retain_not_null(&mut input);

        assert_eq!(input, expected);
    }

    #[test]
    fn int_or_percentage_parse() {
        struct TestCase {
            name: &'static str,
            int_or_string: IntOrString,
            expected: IntOrPercentage,
        }

        impl TestCase {
            fn run(self) {
                let int_or_percentage = IntOrPercentage::try_from(self.int_or_string)
                    .unwrap_or_else(|err| {
                        panic!("Test case '{}' resulted on error: {}", self.name, err);
                    });

                assert_eq!(int_or_percentage, self.expected, "{}", self.name);
            }
        }

        let test_cases = vec![
            TestCase {
                name: "int_or_percentage should parse as int: negative int",
                int_or_string: IntOrString::Int(-100),
                expected: IntOrPercentage::Int(-100),
            },
            TestCase {
                name: "int_or_percentage should parse as int: negative string",
                int_or_string: IntOrString::String("-100".into()),
                expected: IntOrPercentage::Int(-100),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero int",
                int_or_string: IntOrString::Int(0),
                expected: IntOrPercentage::Int(0),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("0".into()),
                expected: IntOrPercentage::Int(0),
            },
            TestCase {
                name: "int_or_percentage should parse as int: positive int",
                int_or_string: IntOrString::Int(100),
                expected: IntOrPercentage::Int(100),
            },
            TestCase {
                name: "int_or_percentage should parse as int: positive string",
                int_or_string: IntOrString::String("100".into()),
                expected: IntOrPercentage::Int(100),
            },
            TestCase {
                name: "int_or_percentage should parse as percent: negative string",
                int_or_string: IntOrString::String("-100%".into()),
                expected: IntOrPercentage::Percentage(-1.0),
            },
            TestCase {
                name: "int_or_percentage should parse as percent: zero string",
                int_or_string: IntOrString::String("0%".into()),
                expected: IntOrPercentage::Percentage(0.0),
            },
            TestCase {
                name: "int_or_percentage should parse as percent: positive string",
                int_or_string: IntOrString::String("100%".into()),
                expected: IntOrPercentage::Percentage(1.0),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn int_or_percentage_parse_error() {
        struct TestCase {
            name: &'static str,
            int_or_string: IntOrString,
        }

        impl TestCase {
            fn run(self) {
                // As we cannot control foreign errors (these errors come from the standard library) we simply test
                // that it fails, but we do not need to know which error.
                let _ = IntOrPercentage::try_from(self.int_or_string).inspect(|ok| {
                    panic!(
                        "Test case '{}' should error and did not. Value returned: {}",
                        self.name, ok
                    )
                });
            }
        }

        let test_cases = vec![
            TestCase {
                name: "int_or_percentage should not parse: random string",
                int_or_string: IntOrString::String("NaN".into()),
            },
            TestCase {
                name: "int_or_percentage should not parse: negative no-string",
                int_or_string: IntOrString::String("-".into()),
            },
            TestCase {
                name: "int_or_percentage should not parse: no-percentage",
                int_or_string: IntOrString::String("%".into()),
            },
            TestCase {
                name: "int_or_percentage should not parse: negative no-percentage",
                int_or_string: IntOrString::String("-%".into()),
            },
            TestCase {
                name: "int_or_percentage should not parse: zero string",
                int_or_string: IntOrString::String("".into()),
            },
            TestCase {
                name: "int_or_percentage should not parse: broken percentage",
                int_or_string: IntOrString::String("%100".into()),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_metadata_name() {
        // As it is a generic, I want to test with at least two different types.
        // Let's start with a Deployment
        let mut deployment = Deployment {
            ..Default::default()
        };
        let deployment_error = get_metadata_name(&deployment).unwrap_err();
        deployment.metadata.name = Some("name".into());
        let deployment_name = get_metadata_name(&deployment).unwrap();
        assert_eq!(
            deployment_error.to_string(),
            Error::MissingName("Deployment".to_string()).to_string()
        );
        assert_eq!(deployment_name, "name".to_string());

        // Now a DaemonSet
        let mut daemon_set = DaemonSet::default();
        let daemon_set_error = get_metadata_name(&daemon_set).unwrap_err();
        daemon_set.metadata.name = Some("name".into());
        let daemon_set_name = get_metadata_name(&daemon_set).unwrap();
        assert_eq!(
            daemon_set_error.to_string(),
            Error::MissingName("DaemonSet".to_string()).to_string()
        );
        assert_eq!(daemon_set_name, "name".to_string());
    }
}
