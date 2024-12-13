use newrelic_agent_control::agent_control::defaults::DYNAMIC_AGENT_TYPE_FILENAME;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn get_agent_type_custom(local_dir: PathBuf, path: &str, args: &str) -> String {
    let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);

    let mut local_file =
        File::create(agent_type_file_path.clone()).expect("failed to create local config file");
    let custom_agent_type = format!(
        r#"
namespace: newrelic
name: com.newrelic.custom_agent
version: 0.1.0
variables:
  on_host:
    backoff_delay:
      description: "seconds until next retry if agent fails to start"
      type: string
      required: false
      default: 1s
deployment:
  on_host:
    executable:
      path: {}
      args: {}
"#,
        path, args
    );
    write!(local_file, "{}", custom_agent_type).unwrap();

    "newrelic/com.newrelic.custom_agent:0.1.0".to_string()
}

pub fn get_agent_type_without_executables(local_dir: PathBuf, health_file_path: &Path) -> String {
    let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);

    let mut local_file =
        File::create(agent_type_file_path.clone()).expect("failed to create local config file");
    let custom_agent_type = format!(
        r#"
namespace: newrelic
name: com.newrelic.custom_agent
version: 0.1.0
variables:
  on_host:
    backoff_delay:
      description: "seconds until next retry if agent fails to start"
      type: string
      required: false
      default: 1s
deployment:
  on_host:
    health:
      interval: 2s
      timeout: 1s
      file:
          path: "{}"
          should_be_present: true
          unhealthy_string: ".*(unhealthy|fatal|error).*"
"#,
        health_file_path.to_str().unwrap()
    );
    write!(local_file, "{}", custom_agent_type).unwrap();

    "newrelic/com.newrelic.custom_agent:0.1.0".to_string()
}
