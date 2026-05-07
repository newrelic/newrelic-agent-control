use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;
use tracing::info;

const CERT_FILENAME: &str = "localhost.crt";
const KEY_FILENAME: &str = "localhost.key";

/// A self-signed TLS certificate for `localhost` that is installed in the system root store.
/// Removes itself from the root store on drop.
pub struct SelfSignedCert {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    _temp_dir: TempDir,
}

impl SelfSignedCert {
    /// Generates a self-signed certificate for `localhost` using `openssl`, adds it to the
    /// system root store, and returns the struct holding paths to the cert and key files.
    pub fn generate() -> Self {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir for cert");
        let cert_path = temp_dir.path().join(CERT_FILENAME);
        let key_path = temp_dir.path().join(KEY_FILENAME);

        generate_cert(&cert_path, &key_path);
        add_to_root_store(&cert_path);

        Self {
            cert_path,
            key_path,
            _temp_dir: temp_dir,
        }
    }
}

fn generate_cert(cert_path: &std::path::Path, key_path: &std::path::Path) {
    info!("Generating self-signed certificate for localhost");

    let key_path_str = key_path.to_string_lossy();
    let cert_path_str = cert_path.to_string_lossy();

    let mut cmd = Command::new("openssl");
    cmd.args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-keyout",
        &key_path_str,
        "-out",
        &cert_path_str,
        "-days",
        "1",
        "-nodes",
        "-subj",
        "/CN=localhost",
        "-addext",
        "subjectAltName=DNS:localhost,IP:127.0.0.1",
    ]);

    #[cfg(target_family = "unix")]
    cmd.args([
        "-addext",
        "basicConstraints=critical,CA:FALSE",
        "-addext",
        "keyUsage=critical,digitalSignature,keyEncipherment",
    ]);

    let output = cmd.output().expect("failed to run openssl");
    assert!(
        output.status.success(),
        "openssl req failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "windows")]
fn add_to_root_store(cert_path: &std::path::Path) {
    info!("Adding certificate to Windows root store");
    let output = Command::new("certutil")
        .args(["-addstore", "-f", "Root", &cert_path.to_string_lossy()])
        .output()
        .expect("failed to run certutil");
    assert!(
        output.status.success(),
        "certutil -addstore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_family = "unix")]
fn add_to_root_store(cert_path: &std::path::Path) {
    info!("Adding certificate to system root store (Linux)");

    // Copy certificate to /usr/local/share/ca-certificates/
    let system_cert_dir = std::path::Path::new("/usr/local/share/ca-certificates");
    let system_cert_path = system_cert_dir.join("localhost-test.crt");

    std::fs::copy(cert_path, &system_cert_path)
        .expect("failed to copy certificate to /usr/local/share/ca-certificates");

    // Update CA certificates
    let output = Command::new("update-ca-certificates")
        .output()
        .expect("failed to run update-ca-certificates");

    assert!(
        output.status.success(),
        "update-ca-certificates failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

impl Drop for SelfSignedCert {
    fn drop(&mut self) {
        #[cfg(target_family = "unix")]
        {
            // Remove certificate from system store on Linux
            let system_cert_path =
                std::path::Path::new("/usr/local/share/ca-certificates/localhost-test.crt");
            let _ = std::fs::remove_file(system_cert_path);
            let _ = Command::new("update-ca-certificates").output();
        }
    }
}
