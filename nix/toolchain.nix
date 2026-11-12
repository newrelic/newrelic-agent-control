# Rust toolchain, crane library and source/args shared by the other modules.
# Exposed to every perSystem module as the `ac` module argument.
{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    let
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ inputs.rust-overlay.overlays.default ];
      };
      inherit (pkgs) lib;

      workspaceToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
      agentControlToml = builtins.fromTOML (builtins.readFile ../agent-control/Cargo.toml);
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
        root = ../.;
        fileset = lib.fileset.unions [
          (craneLib.fileset.commonCargoSources ../.)
          (lib.fileset.fileFilter (f: f.hasExt "yaml") ../agent-control/agent-type-registry)
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
      _module.args = {
        inherit pkgs;
        ac = {
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
    };
}
