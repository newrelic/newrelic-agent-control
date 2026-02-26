use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::Path;
use std::process::Command;

/// Helper function modified to return Result<(), String>
/// This allows .map_err(OCIPackageManagerError::InstallScriptError) to wrap the string.
pub fn install_ebpf_agent(install_path: &Path) -> Result<(), String> {
    // 1. Detect Kernel Version
    let output = Command::new("uname")
        .arg("-r")
        .output()
        .map_err(|e| format!("Failed to check kernel version: {}", e))?;

    if !output.status.success() {
        return Err("uname command failed to execute".into());
    }
    let kernel_version = String::from_utf8_lossy(&output.stdout).trim().to_string();

    println!(
        "🚀 Installing linux headers for kernel version: {}",
        kernel_version
    );

    // 2. Install Linux Headers
    let apt_status = Command::new("apt-get")
        .args([
            "install",
            "-yq",
            "--no-install-recommends",
            &format!("linux-headers-{}", kernel_version),
        ])
        .status()
        .map_err(|e| format!("Failed to execute apt-get: {}", e))?;

    if !apt_status.success() {
        return Err(format!(
            "apt-get exit code was non-zero for kernel {}",
            kernel_version
        ));
    }

    // 3. Move Assets to /lib
    let source_dir = install_path.join("lib").join("newrelic-ebpf-agent");
    let dest_dir = "/lib";

    println!("aa path: {}", source_dir.display());

    if source_dir.exists() {
        println!("📂 Moving assets to {}...", dest_dir);

        fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Could not create directory {}: {}", dest_dir, e))?;

        let mv_status = Command::new("mv")
            .arg(format!("{}", source_dir.display()))
            .arg(dest_dir)
            .status()
            .map_err(|e| format!("Failed to move files: {}", e))?;

        if !mv_status.success() {
            return Err("mv command failed to complete".into());
        }
    }

    // 4. Create Symbolic Link
    let target = "/lib/newrelic-ebpf-agent/nr-ebpf-agent";
    let link = install_path.join("nr-ebpf-agent");

    // Clean up existing link/file
    if link.exists() || fs::symlink_metadata(link.clone()).is_ok() {
        fs::remove_file(link.clone()).map_err(|e| {
            format!(
                "Failed to remove existing file/link {}: {}",
                link.display(),
                e
            )
        })?;
    }

    println!("🔗 Linking {} -> {}", link.display(), target);
    unix_fs::symlink(target, link).map_err(|e| format!("Failed to create symlink: {}", e))?;

    Ok(())
}
