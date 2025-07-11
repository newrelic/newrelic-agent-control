use std::collections::{HashMap, HashSet};

use crate::agent_type::{templates::template_re, variable::namespace::Namespace};

pub type RuntimeVariables = HashMap<String, HashSet<String>>;

pub fn extract_runtime_variables(s: &str) -> RuntimeVariables {
    let mut result = RuntimeVariables::new();

    let re_template = template_re();
    for captures in re_template.captures_iter(s) {
        // "Example with a template: ${nr-var:name|indent 2|to_upper}"
        // templatable_placeholder="${nr-var:name|indent 2|to_upper}"
        // captured_var="nr-var:name"
        // captured_functions="|indent 2|to_upper"
        let (_templatable_placeholder, [captured_var, _captured_functions]) = captures.extract();

        if !Namespace::is_runtime_variable(captured_var) {
            continue;
        }

        let (prefix, var_name) = captured_var
            .split_once(Namespace::PREFIX_NS_SEPARATOR)
            .map(|v| (v.0.to_string(), v.1.to_string()))
            .expect("Namespace format should be valid");
        result.entry(prefix).or_default().insert(var_name);
    }

    result
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_extract_runtime_variables() {
        let input = r#"
data: ${nr-var:var.name|indent 2}
path:${nr-env:PATH_A|indent 2|indent 2}
value: hardcoded value, another_path: ${nr-env:PATH_B}
${nr-env:PATH_C}
eof"#;

        let expected = HashMap::from([(
            "nr-env".to_string(),
            HashSet::from([
                "PATH_A".to_string(),
                "PATH_B".to_string(),
                "PATH_C".to_string(),
            ]),
        )]);
        assert_eq!(extract_runtime_variables(input), expected);
    }

    #[rstest]
    fn test_extract_runtime_variables_when_no_runtime_variables_are_present(
        #[values(
            "test string",
            "${nr-var:var.name}",
            "${nr-var:var.name|indent 2}",
            "${nr-var:var.name|indent 2|indent 2}",
            "${nr-sub:var.name}",
            "${nr-ac:var.name}",
            "${nr-var:var.name|indent 2} ${nr-var:var.name|indent 2} ${nr-var:var.name|indent 2}"
        )]
        input: &str,
    ) {
        assert_eq!(extract_runtime_variables(input), HashMap::new());
    }
}
