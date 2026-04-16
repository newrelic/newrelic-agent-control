use crate::common::on_drop::CleanUp;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::bash::exec_bash_command;
use crate::linux::fake_oci_registry::FakeOciRegistry;
use crate::linux::fake_opamp_server::{FakeOpAMPServer, read_ac_instance_id};
use crate::linux::install::tear_down_test;
use crate::linux::{self, install::install_agent_control_from_recipe};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;

const FAKE_AC_VERSION_NEW: &str = "2.0.0";

pub fn test_agent_control_self_update(args: InstallationArgs) {
    // Start the OpAMP server first so it has time to initialize while the fake binary is compiled.
    let opamp_server = FakeOpAMPServer::start_new();
    info!(endpoint = %opamp_server.endpoint(), "Fake OpAMP server started");

    info!(target_version = FAKE_AC_VERSION_NEW, "Building fake AC binary");
    let fake_binary = build_fake_ac_binary(FAKE_AC_VERSION_NEW);

    info!("Creating tar.gz package from fake binary");
    let tar_gz = create_tar_gz_bytes(&fake_binary);
    info!(size_bytes = tar_gz.len(), "tar.gz package created");

    let registry = FakeOciRegistry::start(FAKE_AC_VERSION_NEW, tar_gz);
    info!(registry_addr = %registry.addr, "Fake OCI registry ready");

    let _clean_up = CleanUp::new(|| {
        tear_down_test();
    });

    let recipe_data = RecipeData {
        args,
        monitoring_source: "none".to_string(),
        fleet_enabled: false,
        ..Default::default()
    };

    info!("Installing Agent Control from recipe");
    install_agent_control_from_recipe(&recipe_data);
    info!("Agent Control installed");

    info!("Configuring Agent Control to use fake OpAMP server and OCI registry");
    write_test_config(&opamp_server.endpoint(), &opamp_server.jwks_endpoint(), &registry.addr);

    info!("Restarting Agent Control service");
    linux::service::restart_service(linux::SERVICE_NAME);

    info!("Waiting for service to become active");
    retry_panic(30, Duration::from_secs(2), "service status check", || {
        linux::service::check_service_is_active(linux::SERVICE_NAME)
    });
    info!("Service is active");

    info!("Waiting for Agent Control instance ID to be written to disk");
    let instance_id = retry_panic(60, Duration::from_secs(2), "read instance_id", || {
        read_ac_instance_id().ok_or_else(|| "instance_id not yet available".into())
    });
    info!("Got instance ID ({} bytes)", instance_id.len());

    info!(
        target_version = FAKE_AC_VERSION_NEW,
        "Delivering new version via OpAMP remote config"
    );
    let config_yaml = format!("version: \"{FAKE_AC_VERSION_NEW}\" \nagents: {{}}");
    let mut opamp_server = opamp_server;
    opamp_server.set_config_response(instance_id, config_yaml);

    info!("Waiting for self-replacement log entry (up to 120 retries x 5s)");
    retry_panic(120, Duration::from_secs(5), "self-replacement", || {
        check_logs_contain_self_replacement()
    });
    info!("Self-replacement detected in logs");

    info!("Waiting 10s for service to restart after self-replacement");
    std::thread::sleep(Duration::from_secs(10));

    info!("Verifying service restarted successfully");
    retry_panic(30, Duration::from_secs(2), "service restart check", || {
        linux::service::check_service_is_active(linux::SERVICE_NAME)
    });

    info!("Waiting 10s for stability check");
    std::thread::sleep(Duration::from_secs(10));

    info!("Final stability check");
    retry_panic(10, Duration::from_secs(2), "stability check", || {
        linux::service::check_service_is_active(linux::SERVICE_NAME)
    });

    info!("Self-update test passed");
}

// Binary creation

/// Builds a fake AC binary that prints its version and runs forever as a daemon.
fn build_fake_ac_binary(version: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("fake-ac-{}", version));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let src_path = temp_dir.join("main.rs");
    let binary_path = temp_dir.join("newrelic-agent-control");

    let src_code = format!(
        r#"fn main() {{
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("--version") {{
        println!("newrelic-agent-control {}");
        return;
    }}

    if args.get(1).map(|s| s.as_str()) == Some("verify") {{
        println!("{{{{\"message\": \"verification successful\"}}}}");
        std::process::exit(0);
    }}

    // Simulate running - sleep forever
    eprintln!("Fake AC {} running...");
    std::thread::sleep(std::time::Duration::from_secs(u64::MAX));
}}
"#,
        version, version
    );

    std::fs::write(&src_path, src_code).expect("Failed to write source");

    let compile_cmd = format!("rustc {} -o {}", src_path.display(), binary_path.display());
    exec_bash_command(&compile_cmd)
        .unwrap_or_else(|err| panic!("Failed to compile fake binary: {}", err));

    info!("Fake binary created at {}", binary_path.display());
    binary_path
}

/// Creates a tar.gz archive containing the binary and returns its bytes.
fn create_tar_gz_bytes(binary_path: &Path) -> Vec<u8> {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for tar.gz");
    let archive_path = temp_dir.path().join("package.tar.gz");

    let tar_cmd = format!(
        "tar -czf {} -C {} {}",
        archive_path.display(),
        binary_path.parent().unwrap().display(),
        binary_path.file_name().unwrap().to_string_lossy()
    );
    exec_bash_command(&tar_cmd).unwrap_or_else(|err| panic!("Failed to create tar.gz: {}", err));

    std::fs::read(&archive_path).expect("Failed to read tar.gz bytes")
}

// Configuration

/// Removes top-level YAML keys managed by the test from a config string,
/// preserving all other fields (e.g. license_key, region).
fn strip_managed_keys(content: &str) -> String {
    let managed_keys = ["self_update:", "version:", "fleet_control:"];
    let mut result = String::new();
    let mut in_managed_block = false;

    for line in content.lines() {
        // Top-level keys start with an alphabetic character or underscore at column 0
        if line.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            in_managed_block = managed_keys.iter().any(|k| line.starts_with(k));
        }
        if !in_managed_block {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

// Path of the systemd environment file required by the service unit (EnvironmentFile=).
// It must exist for the service to start; content can be empty when the recipe hasn't run.
const SYSTEMD_ENV_CONF_PATH: &str = "/etc/newrelic-agent-control/systemd-env.conf";

/// Writes the Agent Control config file, preserving existing fields (e.g. license_key, region)
/// while setting fleet_control (pointing to fake OpAMP) and self_update (pointing to fake OCI).
fn write_test_config(opamp_endpoint: &str, jwks_endpoint: &str, oci_addr: &str) {
    // Ensure the systemd EnvironmentFile exists so the service can start even when the
    // recipe hasn't run (the file is required by the service unit but may contain no entries).
    if !std::path::Path::new(SYSTEMD_ENV_CONF_PATH).exists() {
        if let Some(parent) = std::path::Path::new(SYSTEMD_ENV_CONF_PATH).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(SYSTEMD_ENV_CONF_PATH, "")
            .unwrap_or_else(|err| panic!("Failed to create systemd-env.conf: {err}"));
        info!("Created empty {SYSTEMD_ENV_CONF_PATH} (recipe did not run)");
    }

    let config_path = linux::DEFAULT_AC_CONFIG_PATH;

    let existing = std::fs::read_to_string(config_path).unwrap_or_else(|_| {
        if let Some(parent) = std::path::Path::new(config_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        "server:\n  enabled: true\nagents: {}\n".to_string()
    });

    let base = strip_managed_keys(&existing);

    let new_config = format!(
        "{base}
fleet_control:
  endpoint: {opamp_endpoint}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {jwks_endpoint}
self_update:
  enabled: true
  signature_verification_enabled: false
  package:
    download:
      oci:
        registry: {oci_addr}
        repository: agent-control
"
    );

    std::fs::write(config_path, new_config)
        .unwrap_or_else(|err| panic!("Failed to write config: {err}"));
}

// Verification

fn check_logs_contain_self_replacement() -> TestResult<()> {
    let logs_path = linux::DEFAULT_LOG_PATH;
    let grep_result = std::process::Command::new("grep")
        .arg("-r")
        .arg("self-replacement")
        .arg(logs_path)
        .output();

    match grep_result {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err("No self-replacement logs found yet".into()),
    }
}
