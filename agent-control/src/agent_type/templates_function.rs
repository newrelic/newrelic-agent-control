use regex::Regex;
use std::sync::OnceLock;
use thiserror::Error;

const TEMPLATE_PIPE_FUNCTIONS: &str = r"\\s*|\s*[a-zA-Z]+\s*\d*";

const INDENT_FUNCTION_NAME: &str = "indent";
const INDENT_STR: &str = " ";

fn template_functions_re() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(TEMPLATE_PIPE_FUNCTIONS).unwrap())
}

/// Error type for function parsing and application.
#[derive(Error, Debug)]
pub enum FunctionError {
    #[error("applying function: {0}")]
    Applying(String),
    #[error("parsing function: {0}")]
    Parsing(String),
    #[error("not supported function: {0}")]
    ParsingName(String),
}

/// Trait that defines a function that can be applied to a string value.
pub trait Function {
    /// Applies the function to a given string value.
    fn apply(&self, value: String) -> Result<String, FunctionError>;
    /// Parses a string value to create an instance of the function.
    /// [FunctionError::ParsingName] Error is returned in case the name
    /// doesn't match with the Function name.
    fn parse(value: &str) -> Result<Self, FunctionError>
    where
        Self: Sized;
}

/// Indents each new line with n spaces adding n spaces after each "\n".
#[derive(Debug, PartialEq)]
pub struct Indent(u8);
impl Function for Indent {
    fn apply(&self, value: String) -> Result<String, FunctionError> {
        let indent = format!("\n{}", INDENT_STR.repeat(self.0 as usize));
        Ok(value.replace("\n", indent.as_str()))
    }
    fn parse(value: &str) -> Result<Self, FunctionError> {
        let trimmed_value = value.trim().to_ascii_lowercase();
        let Some(n_parameter) = trimmed_value.strip_prefix(INDENT_FUNCTION_NAME) else {
            return Err(FunctionError::ParsingName(value.to_string()));
        };

        let n = n_parameter.trim().parse().map_err(|_| {
            FunctionError::Parsing(format!("parsing space number parameter: {value}"))
        })?;
        Ok(Indent(n))
    }
}

/// Holds the possible functions that can be applied to a string and will output a string
#[derive(Debug, PartialEq)]
pub enum SupportedFunction {
    Indent(Indent),
}
impl SupportedFunction {
    /// Parses a string of functions and returns a vector of [SupportedFunction].
    /// The string can contain multiple functions separated by a pipe "|".
    pub fn parse_function_list(
        functions_str: &str,
    ) -> Result<Vec<SupportedFunction>, FunctionError> {
        let re_functions = template_functions_re();
        let function_chain =
            re_functions
                .find_iter(functions_str)
                .try_fold(vec![], |mut acc, m| {
                    let name = m.as_str().trim_start_matches("|");
                    let func = SupportedFunction::parse(name)?;
                    acc.push(func);
                    Ok(acc)
                })?;
        Ok(function_chain)
    }
}

impl Function for SupportedFunction {
    fn apply(&self, value: String) -> Result<String, FunctionError> {
        match self {
            SupportedFunction::Indent(indent) => indent.apply(value),
        }
    }
    fn parse(value: &str) -> Result<Self, FunctionError> {
        Ok(Self::Indent(Indent::parse(value)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    #[rstest]
    #[case::indent_zero("|indent 0", 0)]
    #[case::indent("|indent 33", 33)]
    #[case::indent_max("|indent 255", 255)]
    #[case::no_pipe("indent5", 5)]
    #[case::case_insensitive("|InDeNt5", 5)]
    #[case::spaces_1("|indent5", 5)]
    #[case::spaces_2("   |indent5", 5)]
    #[case::spaces_3("|   indent5", 5)]
    #[case::spaces_4("|indent   5", 5)]
    #[case::spaces_5("|indent5   ", 5)]
    fn test_parse_single_indent(#[case] functions_str: &str, #[case] indentation: u8) {
        let functions = SupportedFunction::parse_function_list(functions_str).unwrap();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0], SupportedFunction::Indent(Indent(indentation)))
    }

    #[rstest]
    #[case::no_space("|indent0|indent33|indent255", vec![0,33,255])]
    #[case::no_first_pipe("indent0|indent33|indent255", vec![0,33,255])]
    #[case::multiple_spaces("| indent0|indent 2|indent3 | indent4| indent 5 | ", vec![0,2,3,4,5])]
    fn test_parse_multi_indent(#[case] functions_str: &str, #[case] indentations: Vec<u8>) {
        let functions = SupportedFunction::parse_function_list(functions_str).unwrap();
        assert_eq!(functions.len(), indentations.len());
        for (i, function) in functions.iter().enumerate() {
            assert_eq!(
                function,
                &SupportedFunction::Indent(Indent(indentations[i]))
            );
        }
    }

    #[rstest]
    #[case::line_indent_0("line1\nline2", "|indent 0", "line1\nline2")]
    #[case::line_indent_1("line1\nline2", "|indent 1", "line1\n line2")]
    #[case::line_indent_10("line1\nline2", "|indent 10", "line1\n          line2")]
    #[case::multiline_indent_line("\nline1\nline2\n", "|indent 1", "\n line1\n line2\n ")]
    #[case::empty_string_will_not_be_indented("", "|indent 2", "")]
    #[case::plain_string_will_not_be_indented("foo", "|indent 2", "foo")]
    #[case::multiple_functions("\n\n", "|indent1|indent2|indent3", "\n      \n      ")]
    fn test_transform_indent(
        #[case] input: &str,
        #[case] functions_str: &str,
        #[case] output: &str,
    ) {
        let functions = SupportedFunction::parse_function_list(functions_str).unwrap();
        let final_value = functions
            .iter()
            .try_fold(input.to_string(), |acc, m| m.apply(acc))
            .unwrap();
        assert_eq!(final_value.as_str(), output)
    }

    #[rstest]
    #[case::indent_missing_n("|indent")]
    #[case::indent_n_bigger_than_u8("|indent 500")]
    #[case::indent_n_bigger_than_u8("|indent -1")]
    #[case::indent_n_bigger_than_u8("|indent notAnu8")]
    fn test_parse_fails(#[case] functions_str: &str) {
        let err = SupportedFunction::parse_function_list(functions_str).unwrap_err();
        assert_matches!(err, FunctionError::Parsing(_))
    }
    #[rstest]
    #[case::single("|unknown func")]
    #[case::multiple("|indent2|unknown func")]
    fn test_parse_fails_unkown_names(#[case] functions_str: &str) {
        let err = SupportedFunction::parse_function_list(functions_str).unwrap_err();
        assert_matches!(err, FunctionError::ParsingName(_))
    }
}
