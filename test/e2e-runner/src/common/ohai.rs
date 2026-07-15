use crate::common::test::TestResult;
use std::fs::read_dir;
use std::path::Path;

pub const SHARED_LINUX_FILESYSTEM_DIR: &str = "/var/lib/newrelic-agent-control/shared-filesystem";
pub const SHARED_WINDOWS_FILESYSTEM_DIR: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\shared-filesystem";
pub const EMBEDDED_LINUX_OHI_BINARIES: [&str; 3] = ["nri-flex", "nri-prometheus", "nri-docker"];
pub const EMBEDDED_WINDOWS_OHI_BINARIES: [&str; 5] = [
    "nri-flex.exe",
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
