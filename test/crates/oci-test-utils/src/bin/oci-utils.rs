//! `oci-utils` — push agent packages and agent type definitions to an OCI registry.
//!
//! Intended for manual testing, demos, and one-off onboarding tasks. Mirrors the role
//! of `fake-opamp-server` for the OCI side: an in-tree binary that lets a developer
//! drive `PackagePublisher` by hand instead of only from integration-test code.
//!
//! Not supported software. Distribution is source-only (`publish = false`); run via
//! `cargo run -p oci-test-utils --bin oci-utils -- ...`.
//!
//! Usage:
//!
//! ```text
//! oci-utils [GLOBAL_OPTIONS] push-package    [--media-type tar-gz|zip] --tag <TAG> <FILE>
//! oci-utils [GLOBAL_OPTIONS] push-agent-type <DEFINITION.yaml>
//! ```

use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use flate2::Compression;
use flate2::write::GzEncoder;
use oci_client::Reference;
use oci_test_utils::{
    AgentTypeArtifact, AgentTypeDefinitionMeta, PackageMediaType, PackagePublisher,
};
use tempfile::NamedTempFile;

const DEFAULT_REGISTRY: &str = "localhost:5001";
const DEFAULT_REPOSITORY: &str = "test";
const OCI_UTILS_PASSWORD_ENV: &str = "OCI_UTILS_PASSWORD";
const OCI_UTILS_TOKEN_ENV: &str = "OCI_UTILS_TOKEN";

#[derive(Parser, Debug)]
#[command(
    name = "oci-utils",
    about = "Push agent packages and agent type definitions to an OCI registry.",
    long_about = "An in-tree dev tool. Pushes pre-built agent packages or agent type \
                  definition artifacts to a local or remote OCI registry. Not supported \
                  software — intended for manual testing and demos."
)]
struct Cli {
    /// Registry host. Plain HTTP is used only for localhost:5001 (mirrors HttpsExcept);
    /// everything else uses HTTPS.
    #[arg(long, default_value = DEFAULT_REGISTRY)]
    registry: String,

    /// Repository/namespace within the registry (e.g. `myorg/newrelic-agent`).
    #[arg(long, default_value = DEFAULT_REPOSITORY)]
    repository: String,

    /// Basic-auth username for remote registries.
    #[arg(long)]
    username: Option<String>,

    /// Basic-auth password. Prefer `--password-stdin` or the `OCI_UTILS_PASSWORD`
    /// env var so secrets don't appear in shell history.
    #[arg(long, conflicts_with_all = ["password_stdin", "token", "token_stdin"])]
    password: Option<String>,

    /// Read the basic-auth password from stdin. Mutually exclusive with `--password`
    /// and the token flags.
    #[arg(long, conflicts_with_all = ["password", "token", "token_stdin"])]
    password_stdin: bool,

    /// Bearer-token auth value. Prefer `--token-stdin` or the `OCI_UTILS_TOKEN`
    /// env var. Mutually exclusive with the basic-auth flags.
    #[arg(long, conflicts_with_all = ["username", "password", "password_stdin", "token_stdin"])]
    token: Option<String>,

    /// Read the bearer token from stdin. Mutually exclusive with the basic-auth flags.
    #[arg(long, conflicts_with_all = ["username", "password", "password_stdin", "token"])]
    token_stdin: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Push a pre-built agent package archive under an explicit tag.
    PushPackage {
        /// Archive media type.
        #[arg(long, value_enum, default_value_t = MediaTypeArg::TarGz)]
        media_type: MediaTypeArg,

        /// Tag to push under. Re-running with the same file + tag is idempotent
        /// (same content digest).
        #[arg(long)]
        tag: String,

        /// Pre-built package archive to upload.
        file: PathBuf,
    },

    /// Push an agent type definition. The tag and archive are derived from the
    /// definition YAML's metadata: tag = `<environment-prefix>-<name>-<version>`,
    /// archive = a `tar.gz` containing the definition YAML named `<tag>.yaml`.
    PushAgentType {
        /// Path to the agent type definition YAML.
        definition: PathBuf,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum MediaTypeArg {
    TarGz,
    Zip,
}

impl From<MediaTypeArg> for PackageMediaType {
    fn from(value: MediaTypeArg) -> Self {
        match value {
            MediaTypeArg::TarGz => PackageMediaType::TarGz,
            MediaTypeArg::Zip => PackageMediaType::Zip,
        }
    }
}

/// Resolved auth choice after the precedence rule (bearer > basic > anonymous).
enum AuthChoice {
    Bearer(String),
    Basic { username: String, password: String },
    Anonymous,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            // Print the cause chain so root causes aren't lost.
            let mut source = err.source();
            while let Some(err) = source {
                eprintln!("  caused by: {err}");
                source = err.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let auth = resolve_auth(&cli)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let publisher = build_publisher(runtime.handle().clone(), &cli, auth);

    match cli.command {
        Command::PushPackage {
            media_type,
            tag,
            file,
        } => push_package(&publisher, &file, media_type.into(), &tag),
        Command::PushAgentType { definition } => push_agent_type(&publisher, &definition),
    }
}

fn build_publisher(
    handle: tokio::runtime::Handle,
    cli: &Cli,
    auth: AuthChoice,
) -> PackagePublisher {
    let publisher =
        PackagePublisher::new(handle, cli.registry.clone()).with_repository(cli.repository.clone());
    match auth {
        AuthChoice::Bearer(token) => publisher.with_bearer_auth(&token),
        AuthChoice::Basic { username, password } => publisher.with_basic_auth(&username, &password),
        AuthChoice::Anonymous => publisher,
    }
}

fn resolve_auth(cli: &Cli) -> Result<AuthChoice, Box<dyn Error>> {
    // Precedence: bearer (cli flag → stdin → env) > basic > anonymous.
    let token = read_secret(
        cli.token.as_deref(),
        cli.token_stdin,
        OCI_UTILS_TOKEN_ENV,
        "token",
    )?;
    if let Some(token) = token {
        return Ok(AuthChoice::Bearer(token));
    }

    if let Some(username) = &cli.username {
        let password = read_secret(
            cli.password.as_deref(),
            cli.password_stdin,
            OCI_UTILS_PASSWORD_ENV,
            "password",
        )?;
        let password = password.unwrap_or_default();
        return Ok(AuthChoice::Basic {
            username: username.clone(),
            password,
        });
    }

    Ok(AuthChoice::Anonymous)
}

/// Reads a secret value from the chosen source.
///
/// Precedence within this function (already orthogonal to the bearer-vs-basic
/// choice handled by the caller): explicit flag > stdin > env. Returns `None`
/// when no source provided a value.
fn read_secret(
    flag: Option<&str>,
    from_stdin: bool,
    env_var: &str,
    label: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    if let Some(value) = flag {
        return Ok(Some(value.to_string()));
    }
    if from_stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        let trimmed = buf.trim_end_matches(['\r', '\n']).to_string();
        if trimmed.is_empty() {
            return Err(format!("--{label}-stdin was set but stdin was empty").into());
        }
        return Ok(Some(trimmed));
    }
    if let Ok(value) = std::env::var(env_var)
        && !value.is_empty()
    {
        return Ok(Some(value));
    }
    Ok(None)
}

fn push_package(
    publisher: &PackagePublisher,
    file: &Path,
    media_type: PackageMediaType,
    tag: &str,
) -> Result<(), Box<dyn Error>> {
    if !file.exists() {
        return Err(format!("package file not found: {}", file.display()).into());
    }
    let reference = publisher.push_with_tag(file, media_type, tag);
    print_pushed("package", &reference);
    Ok(())
}

fn push_agent_type(publisher: &PackagePublisher, definition: &Path) -> Result<(), Box<dyn Error>> {
    let yaml = fs::read_to_string(definition).map_err(|e| {
        format!(
            "failed to read definition file {}: {e}",
            definition.display()
        )
    })?;
    let meta = AgentTypeDefinitionMeta::from_yaml_str(&yaml)?;
    let tag = meta.compose_tag()?;

    // Agent Control expects the definition inside the archive to be named
    // `<tag>.yaml`. See agent-control/src/oci/artifact_definitions.rs.
    let archive_filename = format!("{tag}.yaml");
    let archive_bytes = build_agent_type_archive(&archive_filename, yaml.as_bytes())?;

    // The publisher takes a file path, so materialize the archive to a tempfile.
    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(&archive_bytes)?;
    tmp.flush()?;

    let reference = publisher.push_with_tag(tmp.path(), AgentTypeArtifact, &tag);
    print_pushed("agent type", &reference);
    Ok(())
}

/// Builds an in-memory gzipped tar archive containing a single file. Mirrors the
/// helper at `agent-control/src/oci/artifact_definitions.rs::tar_gz_bytes` (test code).
fn build_agent_type_archive(filename: &str, content: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let enc = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, filename, content)?;
    let enc = tar.into_inner()?;
    Ok(enc.finish()?)
}

fn print_pushed(kind: &str, reference: &Reference) {
    println!("pushed {kind}");
    println!("  reference: {reference}");
    if let Some(digest) = reference.digest() {
        println!("  digest:    {digest}");
    }
}
