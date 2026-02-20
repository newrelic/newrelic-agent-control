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

/// Updates the agent control agents config in `config_path` to the specified in `new_content`
pub fn modify_agents_config(config_path: impl AsRef<str>, actual_content: &str, new_content: &str) {
    let config_path = config_path.as_ref();
    let content = fs::read_to_string(config_path).unwrap_or_else(|e| {
        panic!("failed to read configuration file at {config_path:?}: {e}");
    });

    // Replace the valid empty map with an unclosed one to break YAML parsing
    let updated_content = content.replace(actual_content, new_content);

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

pub fn nrdot_config(nrdot_version: &str) -> String {
    format!(
        r#"
version: {nrdot_version}
{NRDOT_CONFIG}
"#
    )
}

const NRDOT_CONFIG: &str = r#"
config:
  extensions:
    health_check:
  
  receivers:
    otlp:
      protocols:
        grpc:
        http:
  
    hostmetrics:
      # Default collection interval is 60s. Lower if you need finer granularity.
      collection_interval: 60s
      scrapers:
        cpu:
          metrics:
            system.cpu.time:
              enabled: false
            system.cpu.utilization:
              enabled: true
        #load:
        memory:
          metrics:
            system.memory.utilization:
              enabled: true
        paging:
          metrics:
            system.paging.utilization:
              enabled: false
            system.paging.faults:
              enabled: false
        filesystem:
          metrics:
            system.filesystem.utilization:
              enabled: true
        disk:
          metrics:
            system.disk.merged:
              enabled: false
            system.disk.pending_operations:
              enabled: false
            system.disk.weighted_io_time:
              enabled: false
        network:
          metrics:
            system.network.connections:
              enabled: false
  
  processors:
    # group system.cpu metrics by cpu
    metricstransform:
      transforms:
        - include: system.cpu.utilization
          action: update
          operations:
            - action: aggregate_labels
              label_set: [ state ]
              aggregation_type: mean
        - include: system.paging.operations
          action: update
          operations:
            - action: aggregate_labels
              label_set: [ direction ]
              aggregation_type: sum
    # remove system.cpu metrics for states
    filter/exclude_cpu_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "interrupt"'
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "nice"'
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "softirq"'
    filter/exclude_memory_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "slab_unreclaimable"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "inactive"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "cached"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "buffered"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "slab_reclaimable"'
    filter/exclude_memory_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.memory.usage" and attributes["state"] == "slab_unreclaimable"'
          - 'metric.name == "system.memory.usage" and attributes["state"] == "inactive"'
    filter/exclude_filesystem_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.utilization" and attributes["type"] == "squashfs"'
    filter/exclude_filesystem_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.usage" and attributes["type"] == "squashfs"'
          - 'metric.name == "system.filesystem.usage" and attributes["state"] == "reserved"'
    filter/exclude_filesystem_inodes_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.inodes.usage" and attributes["type"] == "squashfs"'
          - 'metric.name == "system.filesystem.inodes.usage" and attributes["state"] == "reserved"'
    filter/exclude_system_disk:
      metrics:
        datapoint:
          - 'metric.name == "system.disk.operations" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.merged" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.io" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.io_time" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.operation_time" and IsMatch(attributes["device"], "^loop.*") == true'
    filter/exclude_system_paging:
      metrics:
        datapoint:
          - 'metric.name == "system.paging.usage" and attributes["state"] == "cached"'
          - 'metric.name == "system.paging.operations" and attributes["type"] == "cached"'
    filter/exclude_network:
      metrics:
        datapoint:
          - 'IsMatch(metric.name, "^system.network.*") == true and attributes["device"] == "lo"'
  
    attributes/exclude_system_paging:
      include:
        match_type: strict
        metric_names:
          - system.paging.operations
      actions:
        - key: type
          action: delete
  
    cumulativetodelta:
  
    transform/host:
      metric_statements:
        - context: metric
          statements:
            - set(metric.description, "")
            - set(metric.unit, "")
  
    transform:
      trace_statements:
        - context: span
          statements:
            - truncate_all(span.attributes, 4095)
            - truncate_all(resource.attributes, 4095)
      log_statements:
        - context: log
          statements:
            - truncate_all(log.attributes, 4095)
            - truncate_all(resource.attributes, 4095)
  
    # used to prevent out of memory situations on the collector
    memory_limiter:
      check_interval: 1s
      limit_mib: ${env:NEW_RELIC_MEMORY_LIMIT_MIB:-100}
  
    batch:
  
    resourcedetection:
      detectors: ["system"]
      system:
        hostname_sources: ["os"]
        resource_attributes:
          host.id:
            enabled: true
  
    resourcedetection/cloud:
      detectors: ["gcp", "ec2", "azure"]
      timeout: 2s
      override: true
  
    # Gives OTEL_RESOURCE_ATTRIBUTES precedence over other sources.
    # host.id is set from env whenever the collector is orchestrated by NR Agents.
    resourcedetection/env:
      detectors: ["env"]
      timeout: 2s
      override: true
  
  exporters:
    otlphttp:
      endpoint: ${env:OTEL_EXPORTER_OTLP_ENDPOINT:-https://otlp.nr-data.net}
      headers:
        api-key: ${env:NEW_RELIC_LICENSE_KEY}
  
  service:
    pipelines:
      metrics/host:
        receivers: [hostmetrics]
        processors:
          - memory_limiter
          - metricstransform
          - filter/exclude_cpu_utilization
          - filter/exclude_memory_utilization
          - filter/exclude_memory_usage
          - filter/exclude_filesystem_utilization
          - filter/exclude_filesystem_usage
          - filter/exclude_filesystem_inodes_usage
          - filter/exclude_system_disk
          - filter/exclude_network
          - attributes/exclude_system_paging
          - transform/host
          - resourcedetection
          - resourcedetection/cloud
          - resourcedetection/env
          - cumulativetodelta
          - batch
        exporters: [otlphttp]
      metrics:
        receivers: [otlp]
        processors: [memory_limiter, transform, resourcedetection, resourcedetection/cloud, resourcedetection/env, batch]
        exporters: [otlphttp]
  
    extensions: [health_check]
"#;
