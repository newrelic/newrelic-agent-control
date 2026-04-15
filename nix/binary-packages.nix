# Binary package builds for different targets
{ pkgs, rust }:

let
  inherit (rust) craneLib commonArgs cargoArtifacts;

  # Function to create a package for a specific target
  mkPackage =
    {
      target,
      zigTarget ? null,
      isWindows ? false,
    }:
    let
      # Determine cargo command based on target
      cargoCommand = if zigTarget != null then "zigbuild" else "build";
      zigbuildArgs = if zigTarget != null then "--target ${target}" else "";

      buildArgs = commonArgs // {
        inherit cargoArtifacts;

        # Build commands for each binary
        buildPhaseCargoCommand = ''
          cargo ${cargoCommand} --release --locked \
            ${zigbuildArgs} \
            --bin newrelic-agent-control \
            --bin newrelic-agent-control-cli \
            --bin newrelic-agent-control-k8s \
            --bin newrelic-agent-control-k8s-cli
        '';

        installPhaseCommand = ''
          mkdir -p $out/bin

          ${
            if isWindows then
              ''
                cp target/${target}/release/newrelic-agent-control.exe $out/bin/
                cp target/${target}/release/newrelic-agent-control-cli.exe $out/bin/
                cp target/${target}/release/newrelic-agent-control-k8s.exe $out/bin/
                cp target/${target}/release/newrelic-agent-control-k8s-cli.exe $out/bin/
              ''
            else
              ''
                cp target/${target}/release/newrelic-agent-control $out/bin/
                cp target/${target}/release/newrelic-agent-control-cli $out/bin/
                cp target/${target}/release/newrelic-agent-control-k8s $out/bin/
                cp target/${target}/release/newrelic-agent-control-k8s-cli $out/bin/
              ''
          }
        '';
      };
    in
    craneLib.buildPackage buildArgs;

  # Build binary packages for each target
  packages = {
    x86_64-linux-musl = mkPackage {
      target = "x86_64-unknown-linux-musl";
      zigTarget = "x86_64-unknown-linux-musl";
    };

    aarch64-linux-musl = mkPackage {
      target = "aarch64-unknown-linux-musl";
      zigTarget = "aarch64-unknown-linux-musl";
    };

    x86_64-windows-msvc = mkPackage {
      target = "x86_64-pc-windows-msvc";
      zigTarget = "x86_64-pc-windows-msvc";
      isWindows = true;
    };

    # Native build for current system
    default = mkPackage {
      target =
        if pkgs.stdenv.isLinux then
          if pkgs.stdenv.isAarch64 then "aarch64-unknown-linux-musl" else "x86_64-unknown-linux-musl"
        else if pkgs.stdenv.isDarwin then
          pkgs.stdenv.hostPlatform.config
        else
          throw "Unsupported system: ${pkgs.system}";
      zigTarget =
        if pkgs.stdenv.isLinux then
          if pkgs.stdenv.isAarch64 then "aarch64-unknown-linux-musl" else "x86_64-unknown-linux-musl"
        else
          null;
    };
  };
in
{
  inherit mkPackage packages;
}
