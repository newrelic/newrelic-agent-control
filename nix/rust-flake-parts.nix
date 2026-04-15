# Rust toolchain configuration as a flake-parts module
{
  inputs,
  ...
}:
{
  perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
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
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;

      # Libraries that the compiled program links against
      buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
        pkgs.libiconv
      ];

      # Tools that run on the build machine during compilation
      baseBuildInputs = with pkgs; [
        rustToolchain
        pkg-config
        git
      ];

      # Development tools
      devTools = with pkgs; [
        cargo-watch
        cargo-edit
        cargo-audit
        cargo-llvm-cov
        cargo-deny
      ];

      # Common source filtering
      src = pkgs.lib.cleanSourceWith {
        src = craneLib.path ../.;
        filter =
          path: type:
          (craneLib.filterCargoSources path type)
          || (builtins.match ".*\\.yaml$" path != null)
          || (builtins.match ".*/agent-type-registry/.*" path != null);
      };

      # Package metadata
      agentControlCargoToml = builtins.fromTOML (builtins.readFile ../agent-control/Cargo.toml);

      # Common build arguments
      commonArgs = {
        inherit src;
        strictDeps = true;
        pname = agentControlCargoToml.package.name;
        version = agentControlCargoToml.package.version;
        nativeBuildInputs = baseBuildInputs ++ [
          pkgs.zig
          pkgs.cargo-zigbuild
        ];
        inherit buildInputs;
      };

      # Build dependencies
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in
    {
      # Export these as _module.args so other modules can access them
      _module.args.rust = {
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
      };
    };
}
