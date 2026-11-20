# Agent Control on-host binaries: native `default` for the current system and
# statically linked musl builds (the production targets) on Linux.
_: {
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      ac = config.ac;

      onhostArgs = "--package newrelic_agent_control --bin newrelic-agent-control --bin newrelic-agent-control-cli";

      mkPackage =
        craneLib: extraArgs:
        let
          args = ac.commonArgs // extraArgs // { cargoExtraArgs = onhostArgs; };
        in
        craneLib.buildPackage (
          args
          // {
            cargoArtifacts = craneLib.buildDepsOnly args;
            doCheck = false;
            meta.mainProgram = "newrelic-agent-control";
          }
        );

      muslTargets = {
        x86_64-linux-musl = {
          triple = "x86_64-unknown-linux-musl";
          targetCc = pkgs.pkgsCross.musl64.stdenv.cc;
        };
        aarch64-linux-musl = {
          triple = "aarch64-unknown-linux-musl";
          targetCc = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;
        };
      };

      mkMusl =
        { triple, targetCc }:
        let
          envTriple = lib.replaceStrings [ "-" ] [ "_" ] triple;
          cc = "${targetCc}/bin/${targetCc.targetPrefix}cc";
        in
        mkPackage (ac.mkCraneLib [ triple ]) {
          nativeBuildInputs = ac.commonArgs.nativeBuildInputs ++ [ targetCc ];
          CARGO_BUILD_TARGET = triple;
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
          "CC_${envTriple}" = cc;
          "CARGO_TARGET_${lib.toUpper envTriple}_LINKER" = cc;
        };
    in
    {
      packages = {
        default = mkPackage ac.craneLib { };
      }
      // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (lib.mapAttrs (_: mkMusl) muslTargets);
    };
}
