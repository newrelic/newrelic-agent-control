use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::attributes::{
    check_identifying_attributes_contains_expected, convert_to_vec_key_value,
};
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::oci_signer::OCISigner;
use crate::common::opamp::FakeServer;
use crate::common::remote_config_status::check_latest_remote_config_status;
use crate::common::retry::retry;
use crate::on_host::tools::config::{create_agent_control_config, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_artifact::push_agent_package;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use opamp_client::opamp::proto::any_value::Value;
use std::time::Duration;
use tempfile::{TempDir, tempdir};

#[cfg(not(target_os = "windows"))]
const FILE_LINUX: &str = "sleep60.sh";
#[cfg(not(target_os = "windows"))]
const LINUX_TEMPLATE: &str = r#"#!/bin/bash
if [[ "$1" == "--version" ]]; then
    echo "{VERSION}"
    exit 0
fi
sleep 60
"#;

#[cfg(target_os = "windows")]
const FILE_WINDOWS: &str = "sleep60.ps1";
#[cfg(target_os = "windows")]
const WINDOWS_TEMPLATE: &str = r#"param ([switch]$Version)
if ($Version) {
    Write-Host "{VERSION}"
    exit
}
Start-Sleep -Seconds 60
"#;

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), needs elevated privileges on Windows"]
fn test_install_and_update_agent_remote_package_with_oci_registry() {
    pub const PCK_VERSION_1: &str = "1.0.0";
    pub const PCK_VERSION_2: &str = "2.0.0";

    let signer = OCISigner::start();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let agent_id = "nr-sleep-agent";

    #[cfg(target_os = "windows")]
    let platform = Platform::Windows;
    #[cfg(not(target_family = "windows"))]
    let platform = Platform::Linux;

    let sleep_agent_type = create_agent_type(
        &local_dir,
        agent_id,
        &platform,
        &signer.jwks_url().to_string(),
    );

    // We push the 2 artifacts first version and updated one
    let version = push_testing_package_platform(&platform, PCK_VERSION_1, Some(&signer));
    let updated_version = push_testing_package_platform(&platform, PCK_VERSION_2, Some(&signer));

    let mut opamp_server = FakeServer::start_new();
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    // We create a local config, we are setting the variable fake_variable defined in the
    // custom_agent_type for other tests to set the version.
    // In this test the fn create_agent_type will use this variable in the oci package block
    // to set the pck version:
    //       ...
    //       version: ${{nr-var:fake_variable}}
    create_local_config(
        agent_id.to_string(),
        format!("fake_variable: '{version}'").to_string(),
        local_dir.path().to_path_buf(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agent_a = format!(
        r#"
        agents:
          {agent_id}:
            agent_type: "{sleep_agent_type}"
        "#
    );

    opamp_server.set_config_response(ac_instance_id.clone(), agent_a);

    let sleep_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());

    retry(60, Duration::from_secs(1), || {
        let expected_identifying_attributes = convert_to_vec_key_value(Vec::from([(
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
            Value::StringValue(PCK_VERSION_1.to_string()),
        )]));

        check_identifying_attributes_contains_expected(
            &opamp_server,
            &sleep_instance_id,
            expected_identifying_attributes.clone(),
        )?;

        check_latest_health_status_was_healthy(&opamp_server, &sleep_instance_id)?;

        Ok(())
    });

    // We reuse the same approach as before with the remote setting
    // the fake_variable with the new version
    let sleep_agent_cfg = format!("fake_variable: '{updated_version}'").to_string();
    opamp_server.set_config_response(sleep_instance_id.clone(), sleep_agent_cfg);

    retry(60, Duration::from_secs(1), || {
        let expected_identifying_attributes = convert_to_vec_key_value(Vec::from([(
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
            Value::StringValue(PCK_VERSION_2.to_string()),
        )]));

        check_identifying_attributes_contains_expected(
            &opamp_server,
            &sleep_instance_id,
            expected_identifying_attributes.clone(),
        )?;

        check_latest_health_status_was_healthy(&opamp_server, &sleep_instance_id)?;

        Ok(())
    });
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), needs elevated privileges on Windows"]
fn test_unsigned_artifact_makes_remote_config_fail_with_oci_registry() {
    pub const VERSION: &str = "1.2.3";

    let signer = OCISigner::start();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let agent_id = "nr-sleep-agent";

    #[cfg(target_os = "windows")]
    let platform = Platform::Windows;
    #[cfg(not(target_family = "windows"))]
    let platform = Platform::Linux;

    let sleep_agent_type = create_agent_type(
        &local_dir,
        agent_id,
        &platform,
        &signer.jwks_url().to_string(),
    );

    // Push unsigned package
    let version = push_testing_package_platform(&platform, VERSION, None);

    let mut opamp_server = FakeServer::start_new();
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agents = format!(
        r#"
        agents:
          {agent_id}:
            agent_type: "{sleep_agent_type}"
        "#
    );
    opamp_server.set_config_response(ac_instance_id.clone(), agents);

    let sleep_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());
    // The agent-type use 'fake_variable' to get the agent version
    let sleep_agent_cfg = format!("fake_variable: '{version}'").to_string();
    opamp_server.set_config_response(sleep_instance_id.clone(), sleep_agent_cfg);

    retry(60, Duration::from_secs(1), || {
        // Remote config status should fail because the package is unsigned
        check_latest_remote_config_status(&opamp_server, &sleep_instance_id, |config_status| {
            if config_status.status == RemoteConfigStatuses::Failed as i32
                && config_status
                    .error_message
                    .contains("signature verification failed")
            {
                Ok(())
            } else {
                Err(
                    "Expected RemoteConfig failure because the signature verification failed"
                        .to_string()
                        .into(),
                )
            }
        })?;
        Ok(())
    });
}

enum Platform {
    #[cfg(not(target_os = "windows"))]
    Linux,
    #[cfg(target_os = "windows")]
    Windows,
}

impl Platform {
    fn pkg_type(&self) -> &str {
        match self {
            #[cfg(not(target_os = "windows"))]
            Platform::Linux => "tar",
            #[cfg(target_os = "windows")]
            Platform::Windows => "zip",
        }
    }

    fn filename(&self) -> &str {
        match self {
            #[cfg(not(target_os = "windows"))]
            Platform::Linux => "sleep60.sh",
            #[cfg(target_os = "windows")]
            Platform::Windows => "sleep60.ps1",
        }
    }

    fn shell_info(&self, agent_id: &str) -> (String, Vec<String>, Vec<String>) {
        let file = self.filename();
        let base_dir = format!("${{nr-sub:packages.{agent_id}.dir}}");

        match self {
            #[cfg(not(target_os = "windows"))]
            Platform::Linux => {
                let full_path = format!("{base_dir}/{file}");
                (
                    "/bin/bash".to_string(),
                    vec![full_path.clone()],                  // Main command
                    vec![full_path, "--version".to_string()], // Version command
                )
            }
            #[cfg(target_os = "windows")]
            Platform::Windows => {
                let full_path = format!("{base_dir}\\{file}");
                let base_args = vec![
                    "-NoProfile".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-File".to_string(),
                ];

                let mut run_cmd = base_args.clone();
                run_cmd.push(full_path.clone());

                let mut version_cmd = base_args;
                version_cmd.push(full_path);
                version_cmd.push("-Version".to_string());

                ("powershell.exe".to_string(), run_cmd, version_cmd)
            }
        }
    }
}

fn create_agent_type(
    local_dir: &TempDir,
    agent_id: &str,
    platform: &Platform,
    public_key_url: &str,
) -> String {
    let pkg_type = platform.pkg_type();
    let (shell_path, run_args, version_args) = platform.shell_info(agent_id);

    // Convert Vec<String> to JSON array strings: ["-NoProfile", "-File", "..."]
    let run_args_json = serde_json::to_string(&run_args).unwrap();
    let version_args_json = serde_json::to_string(&version_args).unwrap();

    let packages_config = format!(
        r#"
{agent_id}:
  type: {pkg_type}
  download:
    oci:
      registry: {OCI_TEST_REGISTRY_URL}
      repository: test
      version: ${{nr-var:fake_variable}}
      public_key_url: {public_key_url}
"#
    );

    let executables = format!(
        r#"[
            {{
                "id": "remote-package-sleep",
                "path": "{shell_path}",
                "args": {run_args_json}
            }}
        ]"#
    );

    let version_config = format!(
        r#"
            {{
                "path": "{shell_path}",
                "args": {version_args_json},
                "regex": "\\d+\\.\\d+\\.\\d+"
            }}
        "#
    );

    CustomAgentType::default()
        .with_executables(Some(&executables))
        .with_version(Some(&version_config))
        .with_packages(Some(&packages_config))
        .build(local_dir.path().to_path_buf())
}

/// Push and signs the package containing the platform-specific binary to be used in the custom agent
fn push_testing_package_platform(
    platform: &Platform,
    version: &str,
    signer: Option<&OCISigner>,
) -> String {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();
    let reference = match platform {
        #[cfg(not(target_os = "windows"))]
        Platform::Linux => {
            let path = dir.path().join("layer_digest.tar.gz");
            TestDataHelper::compress_tar_gz(
                tmp_dir_to_compress.path(),
                &path,
                LINUX_TEMPLATE.replace("{VERSION}", version).as_str(),
                FILE_LINUX,
            );
            let (_, reference) = push_agent_package(
                &path,
                OCI_TEST_REGISTRY_URL,
                PackageMediaType::AgentPackageLayerTarGz,
            );
            reference
        }
        #[cfg(target_os = "windows")]
        Platform::Windows => {
            let path = dir.path().join("layer_digest.zip");
            TestDataHelper::compress_zip(
                tmp_dir_to_compress.path(),
                &path,
                WINDOWS_TEMPLATE.replace("{VERSION}", version).as_str(),
                FILE_WINDOWS,
            );
            let (_, reference) = push_agent_package(
                &path,
                OCI_TEST_REGISTRY_URL,
                PackageMediaType::AgentPackageLayerZip,
            );
            reference
        }
    };

    if let Some(signer) = signer {
        signer.sign_artifact(&reference);
    }

    reference.tag().unwrap().to_string()
}
