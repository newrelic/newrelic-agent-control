# Agent Control on-host binaries: native `default` for the current system, and cross-compiled
# builds available from every host: statically linked musl (the production targets) and Windows.
_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      inherit (config) rustLib;

      onhostArgs = lib.concatStringsSep " " [
        "--package newrelic_agent_control"
        "--bin newrelic-agent-control"
        "--bin newrelic-agent-control-cli"
      ];

      mkPackage =
        craneLib: extraArgs:
        let
          args = rustLib.commonArgs // extraArgs // { cargoExtraArgs = onhostArgs; };
        in
        craneLib.buildPackage (
          args
          // {
            cargoArtifacts = craneLib.buildDepsOnly args;
            doCheck = false;
            meta.mainProgram = "newrelic-agent-control";
          }
        );

      # Cross-compiled targets, built with the toolchains of nixpkgs from any host system.
      crossTargets = {
        x86_64-linux-musl = {
          triple = "x86_64-unknown-linux-musl";
          targetCc = pkgs.pkgsCross.musl64.stdenv.cc;
          rustflags = "-C target-feature=+crt-static";
        };
        aarch64-linux-musl = {
          triple = "aarch64-unknown-linux-musl";
          targetCc = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;
          rustflags = "-C target-feature=+crt-static";
        };
        # MinGW (GNU ABI): the MSVC target needs the Microsoft SDK, which Nix cannot provide.
        x86_64-windows-gnu = {
          triple = "x86_64-pc-windows-gnu";
          targetCc = pkgs.pkgsCross.mingwW64.stdenv.cc;
          rustflags = "-L native=${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib";
          # aws-lc's jitterentropy needs <sched.h>, which MinGW only gets from winpthreads.
          cflags = "-I${
            pkgs.pkgsCross.mingwW64.windows.pthreads.dev or pkgs.pkgsCross.mingwW64.windows.pthreads
          }/include";
        };
      };

      mkCross =
        {
          triple,
          targetCc,
          rustflags,
          cflags ? "",
        }:
        let
          envTriple = lib.replaceStrings [ "-" ] [ "_" ] triple;
          cc = "${targetCc}/bin/${targetCc.targetPrefix}cc";
        in
        mkPackage (rustLib.mkCraneLib [ triple ]) {
          nativeBuildInputs = rustLib.commonArgs.nativeBuildInputs ++ [ targetCc ];
          CARGO_BUILD_TARGET = triple;
          CARGO_BUILD_RUSTFLAGS = rustflags;
          # The host stdenv's fixup targets the host's binary format, so cargo strips instead.
          dontFixup = true;
          CARGO_PROFILE_RELEASE_STRIP = "symbols";
          "CFLAGS_${envTriple}" = cflags;
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
          "CC_${envTriple}" = cc;
          "CARGO_TARGET_${lib.toUpper envTriple}_LINKER" = cc;
        };
    in
    {
      packages = {
        default = mkPackage rustLib.craneLib { };
      }
      // lib.mapAttrs (_: mkCross) crossTargets;
    };
}
