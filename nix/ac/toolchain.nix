# Values of the `ac` options (see options.nix): the Rust toolchain, crane and the sources and
# arguments of the builds.
{ inputs, ... }:
{
  perSystem =
    { pkgs, lib, ... }:
    let
      workspaceToml = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
      agentControlToml = builtins.fromTOML (builtins.readFile ../../agent-control/Cargo.toml);
      rustVersion = workspaceToml.workspace.package.rust-version;

      mkToolchain =
        targets:
        pkgs.rust-bin.stable.${rustVersion}.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools"
          ];
          inherit targets;
        };

      mkCraneLib = targets: (inputs.crane.mkLib pkgs).overrideToolchain (_: mkToolchain targets);

      toolchain = mkToolchain [ ];
      craneLib = mkCraneLib [ ];

      # Cargo sources plus the agent type registry embedded by agent-control/build.rs.
      src = lib.fileset.toSource {
        root = ../../.;
        fileset = lib.fileset.unions [
          (craneLib.fileset.commonCargoSources ../../.)
          (lib.fileset.fileFilter (f: f.hasExt "yaml") ../../agent-control/agent-type-registry)
        ];
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        pname = "newrelic-agent-control";
        version = agentControlToml.package.version;
        nativeBuildInputs = [
          pkgs.cmake
          pkgs.pkg-config
        ];
        buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];
      };
    in
    {
      config.ac = {
        inherit
          rustVersion
          toolchain
          mkToolchain
          mkCraneLib
          craneLib
          src
          commonArgs
          ;
      };
    };
}
