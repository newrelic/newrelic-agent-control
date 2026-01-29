use crate::common::file::write;
use serde_yaml::Value;
use std::{fs, io::Write, path::PathBuf};
use tracing::info;

/// Updates the agent control config in `config_path` to include the content specified in `new_content`
pub fn update_config(config_path: impl AsRef<str>, new_content: impl AsRef<str>) {
    let config_path = config_path.as_ref();
    let new_content = new_content.as_ref();
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

    write(config_path, updated_content);
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
pub fn ac_debug_logging_config(log_file_path: &str) -> String {
    format!(
        r#"
log:
  level: debug
  file:
    enabled: true
    path: {log_file_path}
  format:
    target: false
    formatter: pretty
"#
    )
}

pub fn update_config_for_host_id(config_path: &str, test_id: &str) {
    update_config(
        config_path,
        format!(
            r#"
host_id: {test_id}
"#
        ),
    )
}

/// Modifies the agent-control configuration file to enable debug logging and write logs to a file.
pub fn update_config_for_debug_logging(config_path: &str, log_file_path: &str) {
    update_config(config_path, ac_debug_logging_config(log_file_path))
}

/// Writes a file [LOCAL_CONFIG_FILE_NAME] containing the provided `content` in the provided `config_dir`.
pub fn write_agent_local_config(config_dir: &str, content: String) {
    let path = PathBuf::from(config_dir);
    fs::create_dir_all(path.parent().unwrap()).unwrap_or_else(|err| {
        panic!("Error creating local config: {err}");
    });
    write(path, content);
}

/// Replaces all the occurrences of `old` to `new` in the provided `config_path`.
pub fn replace_string_in_file(config_path: &str, old: &str, new: &str) {
    let config_content = fs::read_to_string(config_path)
        .unwrap_or_else(|err| panic!("Could not read {config_path}: {err}"));

    let updated_content = config_content.replace(old, new);

    write(config_path, updated_content);
}

/// Appends `content` to the configuration file in `config_path`
pub fn append_to_config_file(config_path: &str, content: &str) {
    let mut config_file = fs::OpenOptions::new()
        .append(true)
        .open(config_path)
        .unwrap_or_else(|err| {
            panic!("Error opening '{config_path}' file to add content: {err}");
        });
    writeln!(config_file, "{content}").unwrap_or_else(|err| {
        panic!("Error appending content to '{config_path}' file: {err}");
    });
    config_file.sync_data().unwrap_or_else(|err| {
        panic!("Error syncing data to disk for '{config_path}' file: {err}");
    });
}
