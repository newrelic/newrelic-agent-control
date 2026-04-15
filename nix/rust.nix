# Rust toolchain configuration and common build arguments
{ pkgs, crane, rust-overlay }:

let
  # Read the rust-version from Cargo.toml
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  rustVersion = cargoToml.workspace.package.rust-version;

  # Build Rust toolchain with the version from Cargo.toml
  rustToolchain = pkgs.rust-bin.stable.${rustVersion}.default.override {
    extensions = [
      "rust-src"
      "rust-analyzer"
    ];
    targets = [
      "x86_64-unknown-linux-musl"
      "aarch64-unknown-linux-musl"
      "x86_64-pc-windows-msvc"
    ];
  };

  # Crane library for building Rust projects
  craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

  # Libraries that the compiled program links against
  # Note: This project uses rustls + aws-lc-rs (pure Rust crypto) for static builds
  buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
    # macOS-specific frameworks required for Rust std
    pkgs.libiconv
  ];

  # Tools that run on the build machine during compilation
  baseBuildInputs = with pkgs; [
    # Rust toolchain
    rustToolchain

    # Build tools
    pkg-config
    git # Required by build.rs
  ];

  # Development tools (for devShell)
  devTools = with pkgs; [
    cargo-watch
    cargo-edit
    cargo-audit
    cargo-llvm-cov
    cargo-deny
  ];

  # Common source filtering (exclude non-source files)
  src = pkgs.lib.cleanSourceWith {
    src = craneLib.path ../.;
    filter =
      path: type:
      (craneLib.filterCargoSources path type)
      || (builtins.match ".*\\.yaml$" path != null)
      || (builtins.match ".*/agent-type-registry/.*" path != null);
  };

  # Package metadata from agent-control/Cargo.toml
  agentControlCargoToml = builtins.fromTOML (builtins.readFile ../agent-control/Cargo.toml);

  # Common build arguments
  commonArgs = {
    inherit src;
    strictDeps = true;

    # Package metadata
    pname = agentControlCargoToml.package.name;
    version = agentControlCargoToml.package.version;

    nativeBuildInputs = baseBuildInputs ++ [
      pkgs.zig
      pkgs.cargo-zigbuild
    ];
    inherit buildInputs;
  };

  # Build dependencies (cargo fetch / cargo vendor)
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
{
  inherit
    rustToolchain
    rustVersion
    craneLib
    buildInputs
    baseBuildInputs
    devTools
    src
    agentControlCargoToml
    commonArgs
    cargoArtifacts
    ;
}
