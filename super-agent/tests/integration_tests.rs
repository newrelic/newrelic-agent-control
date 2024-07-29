// This is not used in onhost code yet, so we ignore warnings for now
#[cfg_attr(feature = "onhost", allow(dead_code))]
mod common;

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

#[cfg(feature = "onhost")]
mod on_host;

#[cfg(feature = "k8s")]
mod k8s;

#[cfg(test)]
mod tests {
    use regex::Regex;
    // sub level configs
    static HTTP__HEADERS: &str = r"\s*headers\s*:";
    static LOG__FILE: &str = r"\s*file\s*:";

    // root level configs
    static AGENT_DIR: &str = r"\s*agent_dir\s*:";
    static AGENT_TEMP_DIR: &str = r"\s*agent_temp_dir\s*:";
    static BATCH_QUEUE_DEPTH: &str = r"\s*batch_queue_depth\s*:";
    static COLLECTOR_URL: &str = r"\s*collector_url\s*:";
    static COMMAND_CHANNEL_URL: &str = r"\s*command_channel_url\s*:";
    static CONFIG_DIR: &str = r"\s*config_dir\s*:";
    static CUSTOM_PLUGIN_INSTALLATION_DIR: &str = r"\s*custom_plugin_installation_dir\s*:";
    static DEFAULT_INTEGRATIONS_TEMP_DIR: &str = r"\s*default_integrations_temp_dir\s*:";
    static DM_ENDPOINT: &str = r"\s*dm_endpoint\s*:";
    static ENABLE_ELEVATED_PROCESS_PRIV: &str = r"\s*enable_elevated_process_priv\s*:";
    static EVENT_QUEUE_DEPTH: &str = r"\s*event_queue_depth\s*:";
    static FLUENT_BIT_EXE_PATH: &str = r"\s*fluent_bit_exe_path\s*:";
    static FLUENT_BIT_NR_LIB_PATH: &str = r"\s*fluent_bit_nr_lib_path\s*:";
    static FLUENT_BIT_PARSERS_PATH: &str = r"\s*fluent_bit_parsers_path\s*:";
    static HTTP_SERVER_HOST: &str = r"\s*http_server_host\s*:";
    static IDENTITY_URL: &str = r"\s*identity_url\s*:";
    static IGNORE_SYSTEM_PROXY: &str = r"\s*ignore_system_proxy\s*:";
    static LOGGING_BIN_DIR: &str = r"\s*logging_bin_dir\s*:";
    static LOGGING_CONFIGS_DIR: &str = r"\s*logging_configs_dir\s*:";
    static LOGGING_HOME_DIR: &str = r"\s*logging_home_dir\s*:";
    static LOG_FILE: &str = r"\s*log_file\s*:";
    static METRIC_URL: &str = r"\s*metric_url\s*:";
    static OVERIDE_HOST_ROOT: &str = r"\s*overide_host_root\s*:";
    static OVERRIDE_HOST_ETC: &str = r"\s*override_host_etc\s*:";
    static OVERRIDE_HOST_PROC: &str = r"\s*override_host_proc\s*:";
    static OVERRIDE_HOST_SYS: &str = r"\s*override_host_sys\s*:";
    static PASSTHROUGH_ENVIRONMENT: &str = r"\s*passthrough_environment\s*:";
    static PID_FILE: &str = r"\s*pid_file\s*:";
    static PROXY: &str = r"\s*proxy\s*:";
    static SAFE_BIN_DIR: &str = r"\s*safe_bin_dir\s*:";
    static STRIP_COMMAND_LINE: &str = r"\s*strip_command_line\s*:";

    static VALID_CONFIG_1: &str = r#"
config_agent: |    
  log_to_stdout: true
  verbose: 0
  
  custom_attributes:
    environment: production
    department: engineering
  
  enable_process_metrics: true
  enable_storage_metrics: true
  enable_network_metrics: false
"#;

    #[test]
    fn infra_agent_config_scenarios() {
        struct TestCase {
            name: &'static str,
            regex: &'static str,
            hit_cases: Vec<&'static str>,
        }
        impl TestCase {
            fn run(self, valid_configs: Vec<&str>) {
                let re = Regex::new(self.regex).unwrap();
                for config in self.hit_cases {
                    assert!(re.is_match(config), "test case: {}", self.name);
                }
                // asserts that the regex does not match any of the valid configs
                for config in valid_configs {
                    assert!(!re.is_match(config), "test case: {}", self.name);
                }
            }
        }

        let test_cases = vec![
            // sub-level configs
            TestCase {
                name: "log file",
                regex: LOG__FILE,
                hit_cases: vec![
                    r#"
config_agent: |
  log:
    file: /var/log/agent.log
"#,
                ],
            },
            TestCase {
                name: "http headers",
                regex: HTTP__HEADERS,
                hit_cases: vec![
                    r#"
config_agent: |
  http:
    headers:
      key: value
"#,
                ],
            },
            // root level configs
            TestCase {
                name: "fluent bit path",
                regex: FLUENT_BIT_EXE_PATH,
                hit_cases: vec![
                    r#"
config_agent: |
  fluent_bit_exe_path: /usr/bin/fake
"#,
                    r#"
config_agent: fluent_bit_exe_path: /usr/bin/fake
"#,
                    "fluent_bit_exe_path: /usr/bin/fake",
                ],
            },
            TestCase {
                name: "strip command line",
                regex: STRIP_COMMAND_LINE,
                hit_cases: vec![r#"strip_command_line: false"#],
            },
            TestCase {
                name: "collector url",
                regex: COLLECTOR_URL,
                hit_cases: vec![r#"collector_url: http://localhost:1234"#],
            },
            TestCase {
                name: "identity url",
                regex: IDENTITY_URL,
                hit_cases: vec![r#"identity_url: http://localhost:1234"#],
            },
            TestCase {
                name: "metric url",
                regex: METRIC_URL,
                hit_cases: vec![r#"metric_url: http://localhost:1234"#],
            },
            TestCase {
                name: "dm endpoint",
                regex: DM_ENDPOINT,
                hit_cases: vec![r#"dm_endpoint: http://localhost:1234"#],
            },
            TestCase {
                name: "command channel url",
                regex: COMMAND_CHANNEL_URL,
                hit_cases: vec![r#"command_channel_url: http://localhost:1234"#],
            },
            TestCase {
                name: "ignore system proxy",
                regex: IGNORE_SYSTEM_PROXY,
                hit_cases: vec![r#"ignore_system_proxy: true"#],
            },
            TestCase {
                name: "proxy",
                regex: PROXY,
                hit_cases: vec![r#"proxy: http://localhost:1234"#],
            },
            TestCase {
                name: "override host proc",
                regex: OVERRIDE_HOST_PROC,
                hit_cases: vec![r#"override_host_proc: /proc"#],
            },
            TestCase {
                name: "override host sys",
                regex: OVERRIDE_HOST_SYS,
                hit_cases: vec![r#"override_host_sys: /sys"#],
            },
            TestCase {
                name: "override host etc",
                regex: OVERRIDE_HOST_ETC,
                hit_cases: vec![r#"override_host_etc: /etc"#],
            },
            TestCase {
                name: "override host root",
                regex: OVERIDE_HOST_ROOT,
                hit_cases: vec![r#"overide_host_root: /"#],
            },
            TestCase {
                name: "agent dir",
                regex: AGENT_DIR,
                hit_cases: vec![r#"agent_dir: /var/lib/agent"#],
            },
            TestCase {
                name: "safe bin dir",
                regex: SAFE_BIN_DIR,
                hit_cases: vec![r#"safe_bin_dir: /usr/bin"#],
            },
            TestCase {
                name: "config dir",
                regex: CONFIG_DIR,
                hit_cases: vec![r#"config_dir: /etc/agent"#],
            },
            TestCase {
                name: "event queue depth",
                regex: EVENT_QUEUE_DEPTH,
                hit_cases: vec![r#"event_queue_depth: 10e1000"#],
            },
            TestCase {
                name: "batch queue depth",
                regex: BATCH_QUEUE_DEPTH,
                hit_cases: vec![r#"batch_queue_depth: 100e10000"#],
            },
            TestCase {
                name: "enable elevated process priv",
                regex: ENABLE_ELEVATED_PROCESS_PRIV,
                hit_cases: vec![r#"enable_elevated_process_priv: true"#],
            },
            TestCase {
                name: "custom plugin installation dir",
                regex: CUSTOM_PLUGIN_INSTALLATION_DIR,
                hit_cases: vec![r#"custom_plugin_installation_dir: /usr/bin/"#],
            },
            TestCase {
                name: "passthrough environment",
                regex: PASSTHROUGH_ENVIRONMENT,
                hit_cases: vec![r#"passthrough_environment: [SECRET,OTHER_SECRET]"#],
            },
            TestCase {
                name: "logging configs dir",
                regex: LOGGING_CONFIGS_DIR,
                hit_cases: vec![r#"logging_configs_dir: /etc/logging/configs"#],
            },
            TestCase {
                name: "logging bin dir",
                regex: LOGGING_BIN_DIR,
                hit_cases: vec![r#"logging_bin_dir: /usr/bin/logging"#],
            },
            TestCase {
                name: "logging home dir",
                regex: LOGGING_HOME_DIR,
                hit_cases: vec![r#"logging_home_dir: /var/log/logging"#],
            },
            TestCase {
                name: "log file",
                regex: LOG_FILE,
                hit_cases: vec![r#"log_file: /var/log/file"#],
            },
            TestCase {
                name: "fluent bit parsers path",
                regex: FLUENT_BIT_PARSERS_PATH,
                hit_cases: vec![r#"fluent_bit_parsers_path: /etc/fluent-bit/parsers"#],
            },
            TestCase {
                name: "fluent bit nr lib path",
                regex: FLUENT_BIT_NR_LIB_PATH,
                hit_cases: vec![r#"fluent_bit_nr_lib_path: /usr/lib/fluent-bit/nr-lib"#],
            },
            TestCase {
                name: "http server host",
                regex: HTTP_SERVER_HOST,
                hit_cases: vec![r#"http_server_host: "0.0.0.0:8080""#],
            },
            TestCase {
                name: "default integrations temp dir",
                regex: DEFAULT_INTEGRATIONS_TEMP_DIR,
                hit_cases: vec![r#"default_integrations_temp_dir: /tmp/default-integrations"#],
            },
            TestCase {
                name: "agent temp dir",
                regex: AGENT_TEMP_DIR,
                hit_cases: vec![r#"agent_temp_dir: /tmp/agent"#],
            },
            TestCase {
                name: "pid file",
                regex: PID_FILE,
                hit_cases: vec![r#"pid_file: /test/agent"#],
            },
        ];

        for test_case in test_cases {
            test_case.run(vec![VALID_CONFIG_1]);
        }
    }
}
