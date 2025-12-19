use serde_yaml::Value;
use std::fs;
use tracing::info;

/// Updates the agent control config in `config_path` to include the content specified in `new_content`
pub fn update_config(config_path: &str, new_content: &str) {
    // Read the existing configuration file
    let content = fs::read_to_string(config_path).unwrap_or_else(|e| {
        panic!("failed to read configuration file at {config_path:?}: {e}");
    });

    // Parse the YAML configuration
    let config: Value = serde_yaml::from_str(&content).unwrap_or_else(|e| {
        panic!("failed to parse YAML configuration {config_path:?}: {e}");
    });

    // Parse the new content
    let new_config: Value = serde_yaml::from_str(new_content).unwrap_or_else(|e| {
        panic!("failed to merge YAML configuration with content {new_content:?}: {e}");
    });

    // Merge the two configs (new_config overrides config)
    let merged = merge_yaml_mappings(config, new_config);

    // Write the updated config
    let updated_content = serde_yaml::to_string(&merged).unwrap_or_else(|e| {
        panic!("failed to format the updated YAML configuration: {}", e);
    });

    info!("Updating configuration to: \n---\n{}\n---", updated_content);

    fs::write(config_path, updated_content).unwrap_or_else(|e| {
        panic!("failed to write YAML configuration: {}", e);
    });
}

/// Merges two YAML values, with `new` taking precedence over `base`
fn merge_yaml_mappings(base: Value, new: Value) -> Value {
    let mut merged = base;
    if let (Value::Mapping(base_map), Value::Mapping(new_map)) = (&mut merged, new) {
        for (key, value) in new_map {
            base_map.insert(key, value);
        }
    }
    merged
}

/// Return configuration for debug logging as a string
pub fn debug_logging_config(log_file_path: &str) -> String {
    format!(
        r#"
log:
  level: debug
  file:
    enabled: true
    path: {log_file_path}
  format:
    target: true
    formatter: pretty
"#
    )
}

/// Modifies the agent-control configuration file to enable debug logging and write logs to a file.
pub fn update_config_for_debug_logging(config_path: &str, log_file_path: &str) {
    update_config(config_path, &debug_logging_config(log_file_path))
}
