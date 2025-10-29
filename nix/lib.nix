{
  crateExpression =
    craneLib:
    {
      pkgs,
      lib,
      stdenv,
    }:
    let
      src = craneLib.cleanCargoSource ../.;
      commonArgs = {
        inherit src;
        strictDeps = true;
        # Dependencies which need to be build for the current platform
        # on which we are doing the cross compilation. In this case,
        # pkg-config needs to run on the build platform so that the build
        # script can find the location of openssl. Note that we don't
        # need to specify the rustToolchain here since it was already
        # overridden above.
        nativeBuildInputs = [
          # pkg-config
        ]
        ++ lib.optionals stdenv.buildPlatform.isDarwin [
          pkgs.buildPackages.libiconv
        ];
        # Dependencies which need to be built for the platform on which
        # the binary will run. In this case, we need to compile openssl
        # so that it can be linked with our executable.
        buildInputs = [
          # Add additional build inputs here
          # openssl
        ];
      }
      # All this is only to build Agent Control, so we set name/version in the "common args"
      // craneLib.crateNameFromCargoToml { cargoToml = ../agent-control/Cargo.toml; };
      # Build *just* the cargo dependencies, so we can reuse
      # all of that work (e.g. via cachix) when running in CI
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in
    craneLib.buildPackage (
      commonArgs
      // {
        inherit cargoArtifacts;
        # NB: we disable tests since we'll run them all via our CI or from CLI,
        # Besides, need to disable tests requiring network etc etc
        doCheck = false;
      }
    );

  crateExpressionWin =
    craneLib:
    { pkgs, ... }:
    let
      src = craneLib.cleanCargoSource ../.;

      commonArgsWin = {
        inherit src;
        strictDeps = true;
        doCheck = false;

        # fixes issues with aws-lc-sys
        CFLAGS = "-Wno-stringop-overflow -Wno-array-bounds -Wno-restrict"; # ignore some warnings that pop up when cross compiling
        CFLAGS_x86_64-pc-windows-gnu = "-I${pkgs.windows.pthreads}/include"; # fix missing <pthread.h>

        nativeBuildInputs =
          with pkgs;
          [
            buildPackages.cmake
          ]
          ++ lib.optionals stdenv.buildPlatform.isDarwin [
            buildPackages.libiconv
          ];
      }
      # All this is only to build Agent Control, so we set name/version in the "common args"
      // craneLib.crateNameFromCargoToml { cargoToml = ../agent-control/Cargo.toml; };

      cargoArtifactsWin = craneLib.buildDepsOnly commonArgsWin;
    in
    craneLib.buildPackage (
      commonArgsWin
      // {
        cargoArtifacts = cargoArtifactsWin;
      }
    );
}
