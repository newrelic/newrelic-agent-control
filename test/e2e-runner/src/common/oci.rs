use crate::common::InstallationArgs;
use crate::common::cert::SelfSignedCert;
use crate::common::runtime::tokio_runtime;
use crate::common::test::retry;
use oci_client::Reference;
use oci_test_utils::{OCISigner, PackageMediaType, PackagePublisher};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tracing::info;

const REGISTRY_SCRIPT_PATH: &str = "../../tools/oci-registry.sh";
// 5001 is assumed by the helpers to be http so we use 5002
const LOCAL_REGISTRY_PORT: &str = "5002";

pub struct OciRegistry {
    run_process: Child,
    _cert: SelfSignedCert,
}

impl OciRegistry {
    pub fn start() -> Self {
        // AC can only connect to https registries so we generate certificates to enable TLS
        let cert = SelfSignedCert::generate();

        info!("Installing OCI registry");
        run_script_step("install");

        info!(
            "Starting OCI registry on port {LOCAL_REGISTRY_PORT} with tls_cert: '{}' and tls_key: '{}'",
            cert.cert_path.display(),
            cert.key_path.display()
        );

        // Sanitize path for windows
        let cert_path = cert.cert_path.display().to_string().replace("\\", "\\\\");
        let key_path = cert.key_path.display().to_string().replace("\\", "\\\\");

        let run_process = script_command()
            .arg("run")
            .env("PORT", LOCAL_REGISTRY_PORT)
            .env("TLS_CERT", cert_path)
            .env("TLS_KEY", key_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn oci-registry.sh run: {e}"));

        wait_for_registry();
        info!("OCI registry is ready on port {LOCAL_REGISTRY_PORT}");

        Self {
            run_process,
            _cert: cert,
        }
    }

    pub fn url(&self) -> String {
        format!("localhost:{LOCAL_REGISTRY_PORT}")
    }
}

impl Drop for OciRegistry {
    fn drop(&mut self) {
        let _ = self.run_process.kill();
    }
}

fn run_script_step(step: &str) {
    let output = script_command()
        .arg(step)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oci-registry.sh {step}: {e}"));
    assert!(
        output.status.success(),
        "oci-registry.sh {step} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn script_command() -> Command {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_SCRIPT_PATH);

    #[cfg(target_os = "windows")]
    let bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    #[cfg(target_family = "unix")]
    let bash = PathBuf::from("bash");

    let mut cmd = Command::new(bash);
    cmd.arg(script_path);
    cmd
}

fn wait_for_registry() {
    let url = format!("https://localhost:{LOCAL_REGISTRY_PORT}/v2/");
    info!("Checking registry at {url}");
    retry(30, Duration::from_secs(1), "Checking registry", || {
        reqwest::blocking::get(&url)
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.into())
    })
    .expect("OCI registry did not become ready within 30 seconds");
}

/// Holds a pushed and signed AC package. Keeps the JWKS server alive for the test's duration.
pub struct PushedPackage {
    /// OCI image index reference.
    pub reference: Reference,
    /// URL of the JWKS endpoint serving the public signing key.
    pub jwks_url: String,
    _signer: OCISigner,
}

/// Finds, pushes and signs the AC release archive from `args.artifacts_package_dir`.
pub fn push_ac_package(args: &InstallationArgs) -> PushedPackage {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let reference = push_and_sign(args, &args.agent_control_version, &signer);
    let jwks_url = signer.jwks_url().to_string();

    PushedPackage {
        reference,
        jwks_url,
        _signer: signer,
    }
}

/// Same local AC package pushed under several tags, all signed by one shared JWKS endpoint.
pub struct PushedPackages {
    /// URL of the JWKS endpoint serving the public signing key.
    pub jwks_url: String,
    _signer: OCISigner,
}

/// Pushes and signs the same local AC archive under each tag, reusing one signer.
pub fn push_ac_package_with_tags(args: &InstallationArgs, tags: &[&str]) -> PushedPackages {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    for tag in tags {
        push_and_sign(args, tag, &signer);
    }
    let jwks_url = signer.jwks_url().to_string();

    PushedPackages {
        jwks_url,
        _signer: signer,
    }
}

fn push_and_sign(args: &InstallationArgs, tag: &str, signer: &OCISigner) -> Reference {
    let package_dir = args
        .artifacts_package_dir
        .as_deref()
        .expect("local artifacts are expected");

    let (file, media_type) = find_package(package_dir, &args.agent_control_version);

    let publisher = PackagePublisher::new(
        tokio_runtime().handle().clone(),
        format!("localhost:{LOCAL_REGISTRY_PORT}"),
    );

    let reference = publisher.push_with_tag(&file, media_type, tag);
    signer.sign_artifact(&reference);

    info!("AC package pushed and signed: {reference}");

    reference
}

#[cfg(target_os = "windows")]
fn find_package(dir: &Path, version: &str) -> (PathBuf, PackageMediaType) {
    // we currently don't generate the arm64 package.
    let name = format!("newrelic-agent-control_{version}_windows_amd64.zip");
    let path = dir.join(name);
    assert!(path.exists(), "package not found: {}", path.display());
    (path, PackageMediaType::Zip)
}

#[cfg(target_family = "unix")]
fn find_package(dir: &Path, version: &str) -> (PathBuf, PackageMediaType) {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("unsupported arch: {other}"),
    };
    let name = format!("newrelic-agent-control_{version}_linux_{arch}.tar.gz");
    let path = dir.join(name);
    assert!(path.exists(), "package not found: {}", path.display());
    (path, PackageMediaType::TarGz)
}
