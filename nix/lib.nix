rec {
  /*
    mkAgentControl
    A unified builder for the Agent Control crate that parameterises platform
    specifics (Windows / Darwin adjustments) and exposes extension points for
    additional build & native inputs or environment variables.

    Arguments:
      craneLib:  A crane library instance with the desired Rust toolchain.
      pkgs:      The nixpkgs set corresponding to the (possibly cross) build.
      lib:       pkgs.lib for convenience (passed automatically via callPackage).
      stdenv:    The stdenv for platform checks.
      windows?:  Whether to apply the Windows cross‑compilation overrides.
      extraNativeBuildInputs?: Additional build platform dependencies.
      extraBuildInputs?:       Additional target platform dependencies.
      extraEnv?:               Attrset of extra environment variables.
      doCheck?:                Whether to run cargo tests (default false to match previous behaviour).

    Returned derivation builds the crate using crane, reusing dependency artifacts.
  */
  mkAgentControl =
    {
      craneLib,
      pkgs,
      lib,
      stdenv,
      windows ? false,
      extraNativeBuildInputs ? [ ],
      extraBuildInputs ? [ ],
      extraEnv ? { },
    }:
    let
      # Source filtering reused for every build variant.
      unfilteredRoot = ../.;
      src = lib.fileset.toSource {
        root = unfilteredRoot;
        fileset = lib.fileset.unions [
          (craneLib.fileset.commonCargoSources unfilteredRoot)
          (lib.fileset.maybeMissing ../agent-control/agent-type-registry)
          (lib.fileset.maybeMissing ../agent-control/tests)
        ];
      };

      # Base common arguments applied to all builds.
      baseCommonArgs = {
        inherit src;
        strictDeps = true;
        nativeBuildInputs =
          extraNativeBuildInputs
          ++ lib.optionals stdenv.buildPlatform.isDarwin [ pkgs.buildPackages.libiconv ];
        buildInputs = extraBuildInputs;
      }
      // craneLib.crateNameFromCargoToml { cargoToml = ../agent-control/Cargo.toml; };

      # Windows specific overrides (only when requested).
      windowsOverrides = lib.optionalAttrs windows {
        # Suppress warnings / supply pthread headers for aws-lc-sys & related crates.
        CFLAGS = "-Wno-stringop-overflow -Wno-array-bounds -Wno-restrict";
        CFLAGS_x86_64-pc-windows-gnu = "-I${pkgs.windows.pthreads}/include";
        nativeBuildInputs = (baseCommonArgs.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.cmake ];
        NIX_DEBUG = "1"; # verbose build logs can help cross issues.
      };

      commonArgs = baseCommonArgs // windowsOverrides // extraEnv;
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in
    craneLib.buildPackage (
      commonArgs
      // {
        inherit cargoArtifacts;
      }
    );

  /*
    mkPkgs
    Construct a pkgs set either for native or cross compilation.
    Parameters:
      inputs: flake inputs (needs nixpkgs & rust-overlay).
      system: current host system string.
      crossConfig?: target triple (e.g. "x86_64-unknown-linux-musl"). When omitted native build.
  */
  mkPkgs =
    {
      inputs,
      system,
      crossConfig ? null,
    }:
    let
      overlays = [ (import inputs.rust-overlay) ];
    in
    if crossConfig == null then
      import inputs.nixpkgs { inherit system overlays; }
    else
      import inputs.nixpkgs {
        localSystem = system;
        crossSystem = {
          config = crossConfig;
        };
        inherit overlays;
      };

  /*
    rustVersionFromCargo
    Read pinned rust-version from workspace Cargo.toml at repository root.
  */
  rustVersionFromCargo = pkgs: (pkgs.lib.importTOML ../Cargo.toml).workspace.package.rust-version;

  /*
    mkCraneLibForPkgs
    Given pkgs & inputs, produce a craneLib pinned to workspace rust-version.
  */
  mkCraneLibForPkgs =
    { inputs, pkgs }:
    let
      rustVersion = rustVersionFromCargo pkgs;
    in
    (inputs.crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.stable.${rustVersion}.default);

  # Default cross targets (non-Windows) for convenience.
  defaultCrossTargets = {
    cross-linux-aarch64-musl = "aarch64-unknown-linux-musl";
    cross-linux-x86_64-musl = "x86_64-unknown-linux-musl";
  };

  windowsCrossConfig = "x86_64-w64-mingw32";

  /*
    mkAgentControlForTarget
    High-level convenience combining mkPkgs + mkCraneLibForPkgs + mkAgentControl.
    Args:
      inputs, system, crossConfig?, windows?, doCheck?, extraNativeBuildInputs?, extraBuildInputs?, extraEnv?
  */
  mkAgentControlForTarget =
    {
      inputs,
      system,
      crossConfig ? null,
      windows ? false,
      extraNativeBuildInputs ? [ ],
      extraBuildInputs ? [ ],
      extraEnv ? { },
    }:
    let
      pkgs = mkPkgs { inherit inputs system crossConfig; };
      craneLib = mkCraneLibForPkgs { inherit inputs pkgs; };
    in
    pkgs.callPackage (
      {
        pkgs,
        lib,
        stdenv,
      }:
      mkAgentControl {
        inherit
          craneLib
          pkgs
          lib
          stdenv
          windows
          extraNativeBuildInputs
          extraBuildInputs
          extraEnv
          ;
      }
    ) { };
}
