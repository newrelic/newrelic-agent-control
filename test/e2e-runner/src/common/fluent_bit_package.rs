use crate::common::oci::build_tar_gz_package_from_files;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;

// Pinned to a version confirmed present on the New Relic apt pool for the e2e CI host
// (ubuntu-24.04 / noble, amd64). This is a throwaway POC package, not a production artifact,
// so a single hardcoded (version, distro, arch) combination is sufficient.
const FLUENT_BIT_VERSION: &str = "5.0.6";
const FLUENT_BIT_DISTRO: &str = "ubuntu-noble";
const FLUENT_BIT_ARCH: &str = "arm64";

const NR_OUTPUT_PLUGIN_VERSION: &str = "3.7.0";

pub const FLUENT_BIT_BIN_NAME: &str = "fluent-bit";
pub const NR_OUTPUT_PLUGIN_NAME: &str = "out_newrelic.so";
pub const PARSERS_CONF_NAME: &str = "parsers.conf";

/// Downloads the New Relic-hosted Fluent Bit `.deb`, extracts the engine binary and its bundled
/// `parsers.conf`, downloads the New Relic output plugin, and assembles them into a flat
/// `tar.gz` package ready to be pushed and signed to the local OCI registry.
pub fn build_fluentbit_package() -> (tempfile::TempDir, PathBuf) {
    let work_dir =
        tempfile::tempdir().expect("failed to create temp dir for fluent-bit package build");

    let deb_path = work_dir.path().join("fluent-bit.deb");
    download_file(&deb_url(), &deb_path);

    let extract_dir = work_dir.path().join("extracted");
    fs::create_dir_all(&extract_dir).expect("failed to create extraction dir");
    let status = Command::new("dpkg-deb")
        .arg("-x")
        .arg(&deb_path)
        .arg(&extract_dir)
        .status()
        .expect("failed to run dpkg-deb to extract the fluent-bit package");
    assert!(
        status.success(),
        "dpkg-deb -x failed to extract the fluent-bit package"
    );

    let fluent_bit_bin = extract_dir.join("opt/fluent-bit/bin/fluent-bit");
    let parsers_conf = extract_dir.join("etc/fluent-bit/parsers.conf");
    assert!(
        fluent_bit_bin.exists(),
        "fluent-bit binary not found at expected path in the extracted .deb: {}",
        fluent_bit_bin.display()
    );
    assert!(
        parsers_conf.exists(),
        "parsers.conf not found at expected path in the extracted .deb: {}",
        parsers_conf.display()
    );
    set_executable(&fluent_bit_bin);

    let plugin_path = work_dir.path().join(NR_OUTPUT_PLUGIN_NAME);
    download_file(&plugin_url(), &plugin_path);

    build_tar_gz_package_from_files(&[
        (FLUENT_BIT_BIN_NAME, fluent_bit_bin.as_path()),
        (NR_OUTPUT_PLUGIN_NAME, plugin_path.as_path()),
        (PARSERS_CONF_NAME, parsers_conf.as_path()),
    ])
}

fn deb_url() -> String {
    format!(
        "https://download.newrelic.com/infrastructure_agent/linux/apt/pool/main/f/fluent-bit/fluent-bit_{FLUENT_BIT_VERSION}_{FLUENT_BIT_DISTRO}_{FLUENT_BIT_ARCH}.deb"
    )
}

fn plugin_url() -> String {
    format!(
        "https://github.com/newrelic/newrelic-fluent-bit-output/releases/download/v{NR_OUTPUT_PLUGIN_VERSION}/out_newrelic-linux-{FLUENT_BIT_ARCH}-{NR_OUTPUT_PLUGIN_VERSION}.so"
    )
}

fn download_file(url: &str, dest: &Path) {
    info!(url, dest = %dest.display(), "Downloading file");
    let bytes = reqwest::blocking::get(url)
        .unwrap_or_else(|e| panic!("failed to download {url}: {e}"))
        .error_for_status()
        .unwrap_or_else(|e| panic!("failed to download {url}: {e}"))
        .bytes()
        .unwrap_or_else(|e| panic!("failed to read response body from {url}: {e}"));
    fs::write(dest, bytes).unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .unwrap_or_else(|e| panic!("failed to stat {}: {e}", path.display()))
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap_or_else(|e| {
        panic!(
            "failed to set executable permission on {}: {e}",
            path.display()
        )
    });
}
