//! This module provides conditional templating functionality similar to Helm's
//! {{if condition}} [...] {{end}} syntax. It allows sections of a template
//! to be conditionally included or excluded based on variable values or expressions.
use super::definition::Variables;
use super::error::AgentTypeError;
use regex::Regex;
use std::sync::OnceLock;

// Constants for the conditional templating syntax
pub const CONDITIONAL_IF_BEGIN: &str = "{{if ";
pub const CONDITIONAL_ELSE: &str = "{{else}}";
pub const CONDITIONAL_END: &str = "{{end}}";

// Regex to identify conditional blocks in templates
fn conditional_block_re() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| {
        // Using raw string with literal {{ and }} instead of escapes
        Regex::new(r"(?s){{if\s+(.+?)}}(.*?)(?:{{else}}(.*?))?{{end}}").unwrap()
    })
}

/// Evaluates conditional expressions in templates
pub fn evaluate_condition(condition: &str, variables: &Variables) -> Result<bool, AgentTypeError> {
    // Parse simple conditions of the form "variable_name" or "!variable_name"
    let is_negated = condition.starts_with('!');
    let var_name = if is_negated {
        condition.trim_start_matches('!')
    } else {
        condition
    };

    // Check if the variable exists and has a "truthy" value
    let var_exists = variables.contains_key(var_name);
    let var_is_true = if var_exists {
        match variables.get(var_name) {
            Some(var_def) => {
                if let Some(value) = var_def.get_template_value() {
                    // Consider these values as "falsy": empty string, "false", "0", "no", "off"
                    let str_val = value.to_string().to_lowercase();
                    !(str_val.is_empty()
                        || str_val == "false"
                        || str_val == "0"
                        || str_val == "no"
                        || str_val == "off")
                } else {
                    false
                }
            }
            None => false,
        }
    } else {
        false
    };

    // Apply negation if needed
    Ok(if is_negated {
        !var_is_true
    } else {
        var_is_true
    })
}

/// Processes a template string with conditional blocks
pub fn process_conditionals(
    content: String,
    variables: &Variables,
) -> Result<String, AgentTypeError> {
    let re = conditional_block_re();
    let result = content;
    let mut processed = String::new();
    let mut last_end = 0;

    // Find all conditional blocks and process them one at a time
    // This approach avoids the borrowing issues by scanning the original string
    // and building a new result string
    for captures in re.captures_iter(&result) {
        let full_match = captures.get(0).unwrap();
        let match_start = full_match.start();
        let match_end = full_match.end();

        // Add text between the last match and this one
        processed.push_str(&result[last_end..match_start]);

        // Extract condition and blocks
        let condition = captures.get(1).unwrap().as_str();
        let if_block = captures.get(2).unwrap().as_str();
        let else_block = captures.get(3).map(|m| m.as_str()).unwrap_or("");

        // Evaluate condition and add appropriate block
        let condition_result = evaluate_condition(condition, variables)?;
        if condition_result {
            processed.push_str(if_block);
        } else {
            processed.push_str(else_block);
        }

        // Update last_end to continue after this match
        last_end = match_end;
    }

    // Add any remaining text
    processed.push_str(&result[last_end..]);

    // Process any nested conditional blocks that might be in the result
    if re.is_match(&processed) {
        process_conditionals(processed, variables)
    } else {
        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::variable::definition::VariableDefinition;
    use serde_yaml::Number;
    use std::collections::HashMap;

    #[test]
    fn test_evaluate_condition() {
        // Setup test variables
        let mut variables = HashMap::new();
        variables.insert(
            "true_bool".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(true)),
        );
        variables.insert(
            "false_bool".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(false)),
        );
        variables.insert(
            "empty_string".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("".to_string())),
        );
        variables.insert(
            "non_empty_string".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("value".to_string())),
        );
        variables.insert(
            "zero_number".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(Number::from(0))),
        );
        variables.insert(
            "positive_number".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
        );
        variables.insert(
            "string_false".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("false".to_string())),
        );
        variables.insert(
            "string_off".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("off".to_string())),
        );

        // Test condition evaluation
        assert!(evaluate_condition("true_bool", &variables).unwrap());
        assert!(!evaluate_condition("false_bool", &variables).unwrap());
        assert!(!evaluate_condition("empty_string", &variables).unwrap());
        assert!(evaluate_condition("non_empty_string", &variables).unwrap());
        assert!(!evaluate_condition("zero_number", &variables).unwrap());
        assert!(evaluate_condition("positive_number", &variables).unwrap());
        assert!(!evaluate_condition("string_false", &variables).unwrap());
        assert!(!evaluate_condition("string_off", &variables).unwrap());
        assert!(!evaluate_condition("non_existent", &variables).unwrap());

        // Test negated conditions
        assert!(!evaluate_condition("!true_bool", &variables).unwrap());
        assert!(evaluate_condition("!false_bool", &variables).unwrap());
        assert!(evaluate_condition("!empty_string", &variables).unwrap());
        assert!(evaluate_condition("!non_existent", &variables).unwrap());
    }

    #[test]
    fn test_process_conditionals() {
        // Setup test variables
        let mut variables = HashMap::new();
        variables.insert(
            "enable_debug".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(true)),
        );
        variables.insert(
            "scrape_interval".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("15s".to_string())),
        );

        // Test template with if condition
        let template_with_if = "
            config:
              {{if enable_debug}}
              log_level: debug
              verbose: true
              {{end}}
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        let expected_with_if = "
            config:
              log_level: debug
              verbose: true
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        assert_eq!(
            process_conditionals(template_with_if, &variables).unwrap(),
            expected_with_if
        );

        // Test template with if-else condition
        let template_with_if_else = "
            config:
              {{if enable_debug}}
              log_level: debug
              verbose: true
              {{else}}
              log_level: info
              verbose: false
              {{end}}
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        let expected_with_if_else = "
            config:
              log_level: debug
              verbose: true
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        assert_eq!(
            process_conditionals(template_with_if_else.clone(), &variables).unwrap(),
            expected_with_if_else
        );

        // Test nested conditionals
        let nested_conditionals = "
            config:
              {{if enable_debug}}
              log_level: debug
              verbose: true
              {{if scrape_interval}}
              fast_scrape: true
              {{end}}
              {{else}}
              log_level: info
              verbose: false
              {{end}}
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        let expected_nested = "
            config:
              log_level: debug
              verbose: true
              fast_scrape: true
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        assert_eq!(
            process_conditionals(nested_conditionals, &variables).unwrap(),
            expected_nested
        );

        // Test with disabled debug
        variables.insert(
            "enable_debug".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(false)),
        );

        let expected_with_disabled_debug = "
            config:
              log_level: info
              verbose: false
              scrape_interval: ${nr-var:scrape_interval}
        "
        .to_string();

        assert_eq!(
            process_conditionals(template_with_if_else, &variables).unwrap(),
            expected_with_disabled_debug
        );
    }

    #[test]
    fn test_real_world_prometheus_example() {
        // Setup test variables
        let mut variables = HashMap::new();
        variables.insert(
            "enable_debug".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(true)),
        );
        variables.insert(
            "scrape_interval".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("15s".to_string())),
        );

        // Test similar to the Prometheus agent type configuration
        let template = "
            global:
              licenseKey: ${nr-env:NR_LICENSE_KEY}
              cluster: ${nr-env:NR_CLUSTER_NAME}
              {{if enable_debug}}
              # Debug configuration is applied when enable_debug is true
              debug:
                enabled: true
                level: \"debug\"
                logFormat: \"json\"
              {{end}}
              
            prometheus:
              {{if enable_debug}}
              # Additional debug configurations
              config:
                scrape_interval: ${nr-var:scrape_interval}
                evaluation_interval: 15s
                log_level: debug
              {{else}}
              # Standard production configurations
              config:
                scrape_interval: ${nr-var:scrape_interval}
                evaluation_interval: 30s
                log_level: info
              {{end}}
        "
        .to_string();

        let expected_debug_enabled = "
            global:
              licenseKey: ${nr-env:NR_LICENSE_KEY}
              cluster: ${nr-env:NR_CLUSTER_NAME}
              # Debug configuration is applied when enable_debug is true
              debug:
                enabled: true
                level: \"debug\"
                logFormat: \"json\"
              
              
            prometheus:
              # Additional debug configurations
              config:
                scrape_interval: ${nr-var:scrape_interval}
                evaluation_interval: 15s
                log_level: debug
        "
        .to_string();

        assert_eq!(
            process_conditionals(template.clone(), &variables).unwrap(),
            expected_debug_enabled
        );

        // Test with debug disabled
        variables.insert(
            "enable_debug".to_string(),
            VariableDefinition::new(String::default(), true, None, Some(false)),
        );

        let expected_debug_disabled = "
            global:
              licenseKey: ${nr-env:NR_LICENSE_KEY}
              cluster: ${nr-env:NR_CLUSTER_NAME}
              
              
            prometheus:
              # Standard production configurations
              config:
                scrape_interval: ${nr-var:scrape_interval}
                evaluation_interval: 30s
                log_level: info
        "
        .to_string();

        assert_eq!(
            process_conditionals(template, &variables).unwrap(),
            expected_debug_disabled
        );
    }
}
