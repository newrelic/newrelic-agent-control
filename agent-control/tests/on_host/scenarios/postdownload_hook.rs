use crate::common::agent_control::{StartedAgentControl, start_agent_control_with_custom_config};
use crate::common::base_paths::TempBasePaths;
use crate::common::health::{check_latest_health_status, check_latest_health_status_was_healthy};
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::{AgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::{
    AGENT_CONTROL_MODE_ON_HOST, OCI_TEST_REGISTRY_URL,
};
use newrelic_agent_control::opamp::instance_id::InstanceID;
use oci_test_utils::{OCISigner, PackagePublisher};
use std::time::Duration;
use tempfile::tempdir;

const AGENT_ID: &str = "postdownload-test-agent";

const PACKAGE_VERSION_VARIABLE: &str = r#"
package_version:
  description: "OCI package version to download"
  type: "string"
  required: false
  default: "latest"
"#;

// Builds the custom agent type used by these tests, declaring the `package_version` variable.
fn build_agent_type(dirs: &TempBasePaths, packages: &str, executables: &str) -> String {
    CustomAgentType::default()
        .with_variables(PACKAGE_VERSION_VARIABLE)
        .with_executables(Some(executables))
        .with_packages(Some(packages))
        .build(dirs.local_dir())
}

// Starts Agent Control (local config first, then instance ids/remote config) and applies the agent type.
fn start_and_apply(
    dirs: &TempBasePaths,
    opamp_server: &mut FakeServer,
    sleep_agent_type: &str,
    version: &str,
) -> (StartedAgentControl, InstanceID) {
    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_oci_registry(OCI_TEST_REGISTRY_URL)
        .write(dirs.local_dir());

    create_local_config(
        AGENT_ID.to_string(),
        format!("package_version: '{version}'"),
        dirs.local_dir(),
    );

    let agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    let agents = format!(
        r#"
agents:
  {AGENT_ID}:
    agent_type: "{sleep_agent_type}"
"#
    );
    opamp_server.set_config_response(ac_instance_id, agents);

    let sleep_instance_id =
        get_instance_id(&AgentID::try_from(AGENT_ID).unwrap(), dirs.base_paths());

    (agent_control, sleep_instance_id)
}

// Retries until the sub-agent is unhealthy with `last_error` containing `expected_error`.
fn retry_until_unhealthy_with_error(
    opamp_server: &FakeServer,
    sleep_instance_id: &InstanceID,
    expected_error: &str,
    attempts: usize,
) {
    retry(attempts, Duration::from_secs(1), || {
        check_latest_health_status(opamp_server, sleep_instance_id, |status| {
            !status.healthy && status.last_error.contains(expected_error)
        })
    });
}

#[cfg(not(target_os = "windows"))]
const SLEEP_SCRIPT: &str = "sleep_executable.sh";
#[cfg(not(target_os = "windows"))]
const SLEEP_SCRIPT_CONTENT: &str = r#"#!/bin/bash
sleep 60
"#;

#[cfg(target_os = "windows")]
const SLEEP_SCRIPT: &str = "sleep_executable.ps1";
#[cfg(target_os = "windows")]
const SLEEP_SCRIPT_CONTENT: &str = r#"Start-Sleep -Seconds 60
"#;

// Test successful execution of post_download_hook
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_postdownload_hook_successful_execution_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    #[cfg(not(target_os = "windows"))]
    let hook_script = r#"#!/bin/bash
echo "Post-download hook executed successfully"
exit 0
"#;

    #[cfg(target_os = "windows")]
    let hook_script = r#"@echo off
echo Post-download hook executed successfully
exit 0
"#;

    let version = push_package_with_hook_script(&signer, "success_hook", hook_script);

    let packages = create_packages_config(&signer, "success_hook");
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &sleep_instance_id)?;
        Ok(())
    });
}

// Test post_download_hook with non-existent command
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_postdownload_hook_command_not_found_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    let version = push_package_with_hook_script(&signer, "unused", "dummy");

    let packages = create_packages_config_with_nonexistent_command(&signer);
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry_until_unhealthy_with_error(&opamp_server, &sleep_instance_id, "command not found", 60);
}

// Test that stderr output is captured in error message
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_postdownload_hook_stderr_captured_in_error_message_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    let custom_error_msg = "CUSTOM_ERROR_MESSAGE_12345";

    #[cfg(not(target_os = "windows"))]
    let hook_script = format!(
        r#"#!/bin/bash
echo "{}" >&2
exit 1
"#,
        custom_error_msg
    );

    #[cfg(target_os = "windows")]
    let hook_script = format!(
        r#"@echo off
echo {} 1>&2
exit 1
"#,
        custom_error_msg
    );

    let version = push_package_with_hook_script(&signer, "stderr_hook", &hook_script);

    let packages = create_packages_config(&signer, "stderr_hook");
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry_until_unhealthy_with_error(&opamp_server, &sleep_instance_id, custom_error_msg, 60);
}

// Test that a failing post_download_hook prevents agent from starting
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_postdownload_hook_failure_prevents_agent_start_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    #[cfg(not(target_os = "windows"))]
    let hook_script = r#"#!/bin/bash
echo "Post-download hook failed!" >&2
exit 1
"#;

    #[cfg(target_os = "windows")]
    let hook_script = r#"@echo off
echo Post-download hook failed! 1>&2
exit 1
"#;

    let version = push_package_with_hook_script(&signer, "failing_hook", hook_script);

    let packages = create_packages_config(&signer, "failing_hook");
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry_until_unhealthy_with_error(
        &opamp_server,
        &sleep_instance_id,
        "post-download hook execution failed",
        60,
    );
}

// Test post_download_hook timeout (5 minutes)
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), takes long time"]
fn test_postdownload_hook_timeout_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    #[cfg(not(target_os = "windows"))]
    let hook_script = r#"#!/bin/bash
echo "Sleeping for 6 minutes..."
sleep 360
exit 0
"#;

    #[cfg(target_os = "windows")]
    let hook_script = r#"@echo off
echo Sleeping for 6 minutes...
ping -n 361 127.0.0.1 > nul
exit 0
"#;

    let version = push_package_with_hook_script(&signer, "timeout_hook", hook_script);

    let packages = create_packages_config(&signer, "timeout_hook");
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry_until_unhealthy_with_error(&opamp_server, &sleep_instance_id, "timed out", 360);
}

// Test post_download_hook with arguments and environment variables
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_postdownload_hook_with_arguments_and_env_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let dirs = TempBasePaths::default();

    #[cfg(not(target_os = "windows"))]
    let hook_script = r#"#!/bin/bash
# Check if argument was passed
if [ "$1" != "test-arg" ]; then
    echo "Expected argument 'test-arg', got '$1'" >&2
    exit 1
fi

# Check if PACKAGE_DIR is set
if [ -z "$PACKAGE_DIR" ]; then
    echo "PACKAGE_DIR environment variable not set" >&2
    exit 1
fi

# Check if custom env var is set
if [ "$CUSTOM_ENV" != "test-value" ]; then
    echo "Expected CUSTOM_ENV='test-value', got '$CUSTOM_ENV'" >&2
    exit 1
fi

echo "All checks passed"
exit 0
"#;

    #[cfg(target_os = "windows")]
    let hook_script = r#"@echo off
if not "%1"=="test-arg" (
    echo Expected argument 'test-arg', got '%1' 1>&2
    exit /b 1
)

if "%PACKAGE_DIR%"=="" (
    echo PACKAGE_DIR environment variable not set 1>&2
    exit /b 1
)

if not "%CUSTOM_ENV%"=="test-value" (
    echo Expected CUSTOM_ENV='test-value', got '%CUSTOM_ENV%' 1>&2
    exit /b 1
)

echo All checks passed
exit 0
"#;

    let version = push_package_with_hook_script(&signer, "validation_hook", hook_script);

    let packages = create_packages_config_with_args_and_env(&signer, "validation_hook");
    let executables = create_executables_config();
    let sleep_agent_type = build_agent_type(&dirs, &packages, &executables);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let (_agent_control, sleep_instance_id) =
        start_and_apply(&dirs, &mut opamp_server, &sleep_agent_type, &version);

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &sleep_instance_id)?;
        Ok(())
    });
}

fn push_package_with_hook_script(
    signer: &OCISigner,
    script_name: &str,
    script_content: &str,
) -> String {
    use std::fs::File;

    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();

    #[cfg(not(target_os = "windows"))]
    let script_filename = format!("{}.sh", script_name);
    #[cfg(target_os = "windows")]
    let script_filename = format!("{}.bat", script_name);

    let hook_path = source_dir.path().join(&script_filename);
    let sleep_path = source_dir.path().join(SLEEP_SCRIPT);
    std::fs::write(&hook_path, script_content).unwrap();
    std::fs::write(&sleep_path, SLEEP_SCRIPT_CONTENT).unwrap();

    #[cfg(not(target_os = "windows"))]
    let archive_path = {
        use flate2::{Compression, write::GzEncoder};
        let path = archive_dir.path().join("package.tar.gz");
        let tar_gz = File::create(&path).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(&hook_path, &script_filename)
            .unwrap();
        tar.append_path_with_name(&sleep_path, SLEEP_SCRIPT)
            .unwrap();
        tar.finish().unwrap();
        path
    };

    #[cfg(target_os = "windows")]
    let archive_path = {
        use zip::write::SimpleFileOptions;
        let path = archive_dir.path().join("package.zip");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file(&script_filename, options).unwrap();
        let mut hook_file = File::open(&hook_path).unwrap();
        std::io::copy(&mut hook_file, &mut zip).unwrap();

        zip.start_file(SLEEP_SCRIPT, options).unwrap();
        let mut sleep_file = File::open(&sleep_path).unwrap();
        std::io::copy(&mut sleep_file, &mut zip).unwrap();

        zip.finish().unwrap();
        path
    };

    #[cfg(not(target_os = "windows"))]
    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&archive_path, oci_test_utils::PackageMediaType::TarGz);

    #[cfg(target_os = "windows")]
    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&archive_path, oci_test_utils::PackageMediaType::Zip);

    signer.sign_artifact(&reference);

    reference.tag().unwrap().to_string()
}

fn create_packages_config_with_nonexistent_command(signer: &OCISigner) -> String {
    #[cfg(not(target_os = "windows"))]
    let pkg_type = "tar";
    #[cfg(target_os = "windows")]
    let pkg_type = "zip";

    format!(
        r#"
test-package:
  type: {pkg_type}
  download:
    oci:
      repository: test
      version: ${{nr-var:package_version}}
      public_key_url: {}
  post_download_hook:
    path: /this/command/does/not/exist/nowhere
    args:
      - "arg1"
"#,
        signer.jwks_url()
    )
}

fn create_packages_config(signer: &OCISigner, script_name: &str) -> String {
    #[cfg(not(target_os = "windows"))]
    let (hook_shell, hook_args) = {
        let script_path = format!("{}.sh", script_name);
        (
            "/bin/bash",
            format!("    args:\n      - \"{}\"", script_path),
        )
    };

    #[cfg(target_os = "windows")]
    let (hook_shell, hook_args) = {
        let script_path = format!("{}.bat", script_name);
        (
            "cmd.exe",
            format!("    args:\n      - \"/c\"\n      - \"{}\"", script_path),
        )
    };

    #[cfg(not(target_os = "windows"))]
    let pkg_type = "tar";
    #[cfg(target_os = "windows")]
    let pkg_type = "zip";

    format!(
        r#"
test-package:
  type: {pkg_type}
  download:
    oci:
      repository: test
      version: ${{nr-var:package_version}}
      public_key_url: {}
  post_download_hook:
    path: {}
{}
"#,
        signer.jwks_url(),
        hook_shell,
        hook_args
    )
}

fn create_executables_config() -> String {
    #[cfg(not(target_os = "windows"))]
    {
        let script_path = format!("${{nr-sub:packages.test-package.dir}}/{}", SLEEP_SCRIPT);
        format!(
            r#"[
            {{
                "id": "package-sleep",
                "path": "/bin/bash",
                "args": ["{}"]
            }}
        ]"#,
            script_path
        )
    }

    #[cfg(target_os = "windows")]
    {
        let script_path = format!("${{nr-sub:packages.test-package.dir}}\\\\{}", SLEEP_SCRIPT);
        format!(
            r#"[
            {{
                "id": "package-sleep",
                "path": "powershell.exe",
                "args": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "{}"]
            }}
        ]"#,
            script_path
        )
    }
}

fn create_packages_config_with_args_and_env(signer: &OCISigner, script_name: &str) -> String {
    #[cfg(not(target_os = "windows"))]
    let (hook_shell, hook_args) = {
        let script_path = format!("{}.sh", script_name);
        (
            "/bin/bash",
            format!(
                "    args:\n      - \"{}\"\n      - \"test-arg\"",
                script_path
            ),
        )
    };

    #[cfg(target_os = "windows")]
    let (hook_shell, hook_args) = {
        let script_path = format!("{}.bat", script_name);
        (
            "cmd.exe",
            format!(
                "    args:\n      - \"/c\"\n      - \"{}\"\n      - \"test-arg\"",
                script_path
            ),
        )
    };

    #[cfg(not(target_os = "windows"))]
    let pkg_type = "tar";
    #[cfg(target_os = "windows")]
    let pkg_type = "zip";

    format!(
        r#"
test-package:
  type: {pkg_type}
  download:
    oci:
      repository: test
      version: ${{nr-var:package_version}}
      public_key_url: {}
  post_download_hook:
    path: {}
{}
    env:
      CUSTOM_ENV: "test-value"
"#,
        signer.jwks_url(),
        hook_shell,
        hook_args
    )
}
