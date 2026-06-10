//! Build script to generate the agent type registry code, which
//! is then made available to Agent Control at compilation time.
#![warn(missing_docs)]

use glob::glob;
use std::env;
use std::fs;
use std::path::Path;

const REGISTRY_PATH: &str = "agent-type-registry/";
const GENERATED_REGISTRY_FILE: &str = "generated_agent_type_registry.rs";
const GENERATED_PROTOCOL_VERSION_FILE: &str = "generated_protocol_version.rs";

// List of crates whose logs are enabled at the configured level.
const LOGGING_ENABLED_CRATES: &[&str] = &[
    "newrelic_agent_control",
    "self_replacer",
    "resource_detection",
    "fs",
    // External crates (not workspace members) can also be added here freely.
    "opamp_client",
    "nr_auth",
];

// Workspace crates whose logs are explicitly disabled.
// Every workspace member must appear in exactly one of the two lists — the build
// fails otherwise, forcing an explicit decision when a new crate is added.
const LOGGING_DISABLED_CRATES: &[&str] = &[
    "wrapper_with_default",
    "e2e_runner",
    "fake_opamp_server",
    "oci_test_utils",
];

fn main() {
    generate_agent_type_registry();
    generate_protocol_version_const();
    check_logging_crates();
    // setup the env variable for the generated registry path
    println!("cargo:rustc-env=GENERATED_REGISTRY_FILE={GENERATED_REGISTRY_FILE}");
    println!("cargo:rustc-env=GENERATED_PROTOCOL_VERSION_FILE={GENERATED_PROTOCOL_VERSION_FILE}");
    // re-run only if the registry has changed
    println!("cargo:rerun-if-changed={REGISTRY_PATH}");
    set_git_commit();
    set_version();
}

fn set_version() {
    // If present NR_RELEASE_TAG env var will define the AC version, if not it will be taken from
    // the agent-contro/cargo.toml version.
    // This is useful to build identify nightly releases and perform e2e testing.
    println!("cargo:rerun-if-env-changed=NR_RELEASE_TAG");
    let version = env::var("NR_RELEASE_TAG")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set"));
    println!("cargo:rustc-env=AGENT_CONTROL_VERSION={version}");
}

fn set_git_commit() {
    // Re-run whenever the commit or index changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let value = if dirty {
        format!("{commit}-dirty")
    } else {
        commit
    };

    println!("cargo:rustc-env=GIT_COMMIT={value}");
}

/// Checks that all workspace members are listed in either LOGGING_ENABLED_CRATES or LOGGING_DISABLED_CRATES.
/// This forces an explicit decision about logging for each crate, making it less likely that a new crate is added
/// without considering its logging configuration.
fn check_logging_crates() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let workspace_root = Path::new(&manifest_dir).join("..");

    let workspace_toml_path = workspace_root.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", workspace_toml_path.display());

    let workspace_toml: toml::Table = fs::read_to_string(&workspace_toml_path)
        .expect("Could not read workspace Cargo.toml")
        .parse()
        .expect("Could not parse workspace Cargo.toml");

    let members = workspace_toml["workspace"]["members"]
        .as_array()
        .expect("workspace.members is not an array");

    for member in members {
        let member_path = member.as_str().expect("workspace member is not a string");
        let member_toml_path = workspace_root.join(member_path).join("Cargo.toml");
        println!("cargo:rerun-if-changed={}", member_toml_path.display());

        let member_toml: toml::Table = fs::read_to_string(&member_toml_path)
            .unwrap_or_else(|_| panic!("Could not read {member_path}/Cargo.toml"))
            .parse()
            .unwrap_or_else(|_| panic!("Could not parse {member_path}/Cargo.toml"));

        let package_name = member_toml["package"]["name"]
            .as_str()
            .unwrap_or_else(|| panic!("package.name not found in {member_path}/Cargo.toml"));

        // Cargo converts hyphens to underscores in crate names; tracing targets use the crate name form.
        let crate_name = package_name.replace('-', "_");

        let in_enabled = LOGGING_ENABLED_CRATES.contains(&crate_name.as_str());
        let in_disabled = LOGGING_DISABLED_CRATES.contains(&crate_name.as_str());

        if !in_enabled && !in_disabled {
            panic!(
                "\n\nWorkspace crate `{crate_name}` is not listed in LOGGING_ENABLED_CRATES or \
                LOGGING_DISABLED_CRATES in build.rs.\n\
                Add it to one of the lists to make an explicit logging decision.\n"
            );
        }
    }

    let crates_value = LOGGING_ENABLED_CRATES.join(",");
    println!("cargo:rustc-env=LOGGING_ENABLED_CRATES={crates_value}");
}

/// Reads `package.metadata.agent_type_protocol_version` from this crate's `Cargo.toml`,
/// validates that it is a `MAJOR.MINOR` numeric string, and emits a generated `.rs` file
/// defining `SUPPORTED_PROTOCOL_VERSION`, and included by `agent_type::protocol_version`.
fn generate_protocol_version_const() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let manifest: toml::Table = fs::read_to_string(&manifest_path)
        .expect("Could not read agent-control Cargo.toml")
        .parse()
        .expect("Could not parse agent-control Cargo.toml");

    let raw = manifest
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("agent_type_protocol_version"))
        .and_then(|v| v.as_str())
        .expect(
            "package.metadata.agent_type_protocol_version is missing or not a string in agent-control/Cargo.toml",
        );

    let (major, minor) = parse_major_minor(raw).unwrap_or_else(|| {
        panic!(
            "package.metadata.agent_type_protocol_version = \"{raw}\" is not a valid MAJOR.MINOR \
            numeric string"
        )
    });

    let contents = format!(
        "/// Maximum protocol version this Agent Control supports.
pub const SUPPORTED_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion {{ major: {major}, minor: {minor} }};\n"
    );

    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR not set");
    let dest = Path::new(&out_dir).join(GENERATED_PROTOCOL_VERSION_FILE);
    fs::write(dest, contents).expect("Could not write generated protocol_version file");
}

fn parse_major_minor(s: &str) -> Option<(u64, u64)> {
    let (major, minor) = s.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn generate_agent_type_registry() {
    let current_dir =
        env::current_dir().expect("Could not get current directory to embed registry files");
    let registry_paths =
        glob(format!("{REGISTRY_PATH}**/*.yaml").as_str()).expect("could not iter registry files");

    // comma-separated `include_bytes!(<full-path>)` for each file in the registry
    let static_array_content = registry_paths
        .map(|entry| {
            let path = entry.expect("Could not read matching registry file");
            let full_path = Path::new(&current_dir).join(path).display().to_string();
            format!("include_bytes!(r\"{full_path}\")")
        })
        .collect::<Vec<_>>()
        .join(", ");

    // build generated file content
    let contents =
        format!("pub const AGENT_TYPE_REGISTRY_FILES: &[&[u8]] = &[{static_array_content}];");

    let out_dir = env::var_os("OUT_DIR").expect("Could not load the target registry file path");
    let dest_path = Path::new(&out_dir).join(GENERATED_REGISTRY_FILE);
    fs::write(dest_path, contents).expect("Could not write the filesystem registry file");
}
