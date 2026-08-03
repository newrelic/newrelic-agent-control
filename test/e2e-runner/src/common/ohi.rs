use crate::common::InstallationArgs;
use crate::common::config::DEBUG_LOGGING_CONFIG;
use crate::common::config::{update_config, write_agent_local_config};
use crate::common::test::TestResult;
use std::fs::read_dir;
use std::path::Path;

pub const SHARED_LINUX_FILESYSTEM_DIR: &str = "/var/lib/newrelic-agent-control/shared-filesystem";
pub const SHARED_WINDOWS_FILESYSTEM_DIR: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\shared-filesystem";
pub const EMBEDDED_LINUX_OHI_BINARIES: [&str; 2] = ["nri-prometheus", "nri-docker"];
pub const EMBEDDED_WINDOWS_OHI_BINARIES: [&str; 4] = [
    "nri-prometheus.exe",
    "nr-winpkg.exe",
    "nri-winservices.exe",
    "windows_exporter.exe",
];

pub const EMBEDDED_LINUX_OHI_CONFIGS: [&str; 1] = ["docker-config.yml"];

pub fn check_ohi_shared_filesystem(
    base_path: &str,
    binaries: &[&str],
    configs: &[&str],
) -> TestResult<()> {
    const SHARED_OHI_BINARIES_DIR: &str = "infra-agent-ohi-binaries";
    const SHARED_OHI_CONFIGS_DIR: &str = "infra-agent-ohi-configs";

    let binaries_dir = Path::new(base_path).join(SHARED_OHI_BINARIES_DIR);
    for binary in binaries {
        let path = binaries_dir.join(binary);
        if !path.is_file() {
            return Err(format!("expected OHI binary not found at {}", path.display()).into());
        }
    }

    if read_dir(&binaries_dir).unwrap().count() != binaries.len() {
        return Err(format!(
            "expected {} files in binaries shared filesystem, found {}",
            binaries.len(),
            read_dir(&binaries_dir).unwrap().count()
        )
        .into());
    }

    let ohi_configs_dir = Path::new(base_path).join(SHARED_OHI_CONFIGS_DIR);
    for config in configs {
        let path = ohi_configs_dir.join(config);
        if !path.is_file() {
            return Err(format!("expected OHI config not found at {}", path.display()).into());
        }
    }

    if read_dir(&ohi_configs_dir).unwrap().count() != configs.len() {
        return Err(format!(
            "expected {} files in configs shared filesystem, found {}",
            configs.len(),
            read_dir(&ohi_configs_dir).unwrap().count()
        )
        .into());
    }

    Ok(())
}

/// A single OHI's plumbing for this scenario: everything needed to wire the sub-agent, render its
/// integration config, and grep the AC log for its invocation.
pub struct Ohi {
    /// name of the integration.
    pub name: &'static str,
    /// Full agent-type reference to install.
    pub agent_type_id: &'static str,
    /// OCI package version to pin.
    pub version: String,
}

pub fn get_all_ohi_to_test(args: &InstallationArgs) -> Vec<Ohi> {
    let redis_version = args
        .redis_version
        .clone()
        .expect("--redis-version is required for this scenario");
    let nginx_version = args
        .nginx_version
        .clone()
        .expect("--nginx-version is required for this scenario");
    let apache_version = args
        .apache_version
        .clone()
        .expect("--apache-version is required for this scenario");
    let memcached_version = args
        .memcached_version
        .clone()
        .expect("--memcached-version is required for this scenario");

    vec![
        Ohi {
            name: "nri-redis",
            agent_type_id: "newrelic/com.newrelic.infrastructure.nri_redis:0.1.0",
            version: redis_version,
        },
        Ohi {
            name: "nri-nginx",
            agent_type_id: "newrelic/com.newrelic.infrastructure.nri_nginx:0.1.0",
            version: nginx_version,
        },
        Ohi {
            name: "nri-apache",
            agent_type_id: "newrelic/com.newrelic.infrastructure.nri_apache:0.1.0",
            version: apache_version,
        },
        Ohi {
            name: "nri-memcached",
            agent_type_id: "newrelic/com.newrelic.infrastructure.nri_memcached:0.1.0",
            version: memcached_version,
        },
    ]
}

pub const TEST_LABEL: &str = "test.label";
pub const TEST_LABEL_VALUE: &str = "1.2.3";

/// No service is hit by ohis but the infra-agent still logs
/// the invocation with the label, which is what we assert on.
pub fn update_infra_configs_for_ohis_without_service(infra_agent_version: &str, ohis: &[Ohi]) {
    // AC-level config: infra-agent + one sub-agent per OHI.
    let agents_block = ohis
        .iter()
        .map(|o| format!("  {}:\n    agent_type: \"{}\"\n", o.name, o.agent_type_id,))
        .collect::<String>();

    #[cfg(target_family = "unix")]
    let config_path = crate::linux::DEFAULT_AC_CONFIG_PATH;
    #[cfg(target_family = "windows")]
    let config_path = crate::windows::DEFAULT_AC_CONFIG_PATH;
    update_config(
        config_path,
        format!(
            r#"
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
{agents_block}
agent_packages:
  signature_verification_enabled: false
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    #[cfg(target_family = "unix")]
    let config_path = crate::linux::local_config_path("nr-infra");
    #[cfg(target_family = "windows")]
    let config_path = crate::windows::local_config_path("nr-infra");

    write_agent_local_config(
        &config_path,
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  log:
    level: debug
version: {infra_agent_version}
"#
        ),
    );

    for ohi in ohis {
        #[cfg(target_family = "unix")]
        let config_path = crate::linux::local_config_path(ohi.name);
        #[cfg(target_family = "windows")]
        let config_path = crate::windows::local_config_path(ohi.name);

        write_agent_local_config(
            &config_path,
            format!(
                r#"
config:
  integrations:
    - name: {}
      interval: 15s
      labels:
        {}: {}
version: {}
"#,
                ohi.name, TEST_LABEL, TEST_LABEL_VALUE, ohi.version,
            ),
        );
    }
}
