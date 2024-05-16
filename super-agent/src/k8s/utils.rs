use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

/// This is a helper to have the number of pods or percentages for update strategies.
///
/// You can get this enum from `IntOrString` but it could return an error if it is not parsable.
/// `IntOrString` is used on other parts of the API like pod ports (80 or "http") so casting it
/// is not safe from errors if incorrectly used.
///
/// ```
/// use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
/// use newrelic_super_agent::k8s::utils::IntOrPercentage;
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
    /// use newrelic_super_agent::k8s::utils::IntOrPercentage;

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
            IntOrPercentage::Int(i) => write!(f, "{}", i),
            IntOrPercentage::Percentage(float) => {
                let percent = (*float * 100.0) as i32;
                write!(f, "{}%", percent)
            }
        }
    }
}

/// This function returns true if there are labels and they contain the provided key, value.
pub fn contains_label_with_value(
    labels: &Option<BTreeMap<String, String>>,
    key: &str,
    value: &str,
) -> bool {
    labels
        .as_ref()
        .and_then(|labels| labels.get(key))
        .map_or(false, |v| v.as_str() == value)
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn int_or_percentage_parse_int() {
        struct TestCase {
            name: &'static str,
            int_or_string: IntOrString,
            expected: i32,
        }

        impl TestCase {
            fn run(self) {
                let IntOrPercentage::Int(int_or_percentage) =
                    IntOrPercentage::try_from(self.int_or_string).unwrap_or_else(|err| {
                        panic!("Test case '{}' resulted on error: {}", self.name, err)
                    })
                else {
                    panic!("Test case '{}' parsed to percentage", self.name)
                };

                assert_eq!(int_or_percentage, self.expected, "{}", self.name);
            }
        }

        let test_cases = vec![
            TestCase {
                name: "int_or_percentage should parse as int: negative int",
                int_or_string: IntOrString::Int(-100),
                expected: -100,
            },
            TestCase {
                name: "int_or_percentage should parse as int: negative string",
                int_or_string: IntOrString::String("-100".into()),
                expected: -100,
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero int",
                int_or_string: IntOrString::Int(0),
                expected: 0,
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("0".into()),
                expected: 0,
            },
            TestCase {
                name: "int_or_percentage should parse as int: positive int",
                int_or_string: IntOrString::Int(100),
                expected: 100,
            },
            TestCase {
                name: "int_or_percentage should parse as int: positive string",
                int_or_string: IntOrString::String("100".into()),
                expected: 100,
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn int_or_percentage_parse_percentage() {
        struct TestCase {
            name: &'static str,
            int_or_string: IntOrString,
            expected: f32,
        }

        impl TestCase {
            fn run(self) {
                let IntOrPercentage::Percentage(int_or_percentage) =
                    IntOrPercentage::try_from(self.int_or_string).unwrap_or_else(|err| {
                        panic!("Test case '{}' resulted on error: {}", self.name, err)
                    })
                else {
                    panic!("Test case '{}' parsed to integer", self.name)
                };

                assert_eq!(int_or_percentage, self.expected, "{}", self.name);
            }
        }

        let test_cases = vec![
            TestCase {
                name: "int_or_percentage should parse as int: negative string",
                int_or_string: IntOrString::String("-100%".into()),
                expected: -1.0,
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("0%".into()),
                expected: 0.0,
            },
            TestCase {
                name: "int_or_percentage should parse as int: positive string",
                int_or_string: IntOrString::String("100%".into()),
                expected: 1.0,
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
                // that it fails but we do not need to know which error.
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
                name: "int_or_percentage should parse as int: negative string",
                int_or_string: IntOrString::String("NaN".into()),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("%".into()),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("-%".into()),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("".into()),
            },
            TestCase {
                name: "int_or_percentage should parse as int: zero string",
                int_or_string: IntOrString::String("%100".into()),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_contains_label_with_value() {
        struct TestCase {
            name: &'static str,
            labels: Option<BTreeMap<String, String>>,
            key: &'static str,
            value: &'static str,
            expected: bool,
        }

        impl TestCase {
            fn run(self) {
                assert_eq!(
                    self.expected,
                    contains_label_with_value(&self.labels, self.key, self.value),
                    "{}",
                    self.name
                )
            }
        }

        let test_cases = [
            TestCase {
                name: "No labels",
                labels: None,
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Empty labels",
                labels: Some(BTreeMap::default()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "No matching label",
                labels: Some([("a".to_string(), "b".to_string())].into()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Matching label with different value",
                labels: Some([("key".to_string(), "other".to_string())].into()),
                key: "key",
                value: "value",
                expected: false,
            },
            TestCase {
                name: "Matching label and value",
                labels: Some([("key".to_string(), "value".to_string())].into()),
                key: "key",
                value: "value",
                expected: true,
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
