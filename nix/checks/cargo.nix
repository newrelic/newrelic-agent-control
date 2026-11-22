# Cargo steps of CI that `nix flake check` runs: rustfmt and clippy.
# treefmt and git-hooks contribute their own checks from their modules.
_: {
  perSystem =
    { config, ... }:
    let
      inherit (config) rustLib;
      inherit (rustLib) craneLib;
      workspaceArgs = rustLib.commonArgs // {
        cargoExtraArgs = "--workspace";
        # Tests rely on debug assertions, as in CI.
        CARGO_PROFILE = "dev";
      };
    in
    {
      checks = {
        cargo-clippy = craneLib.cargoClippy (
          workspaceArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly workspaceArgs;
            cargoClippyExtraArgs = "--all-targets -- -D warnings -D clippy::all";
          }
        );

        cargo-fmt = craneLib.cargoFmt {
          inherit (rustLib) src;
          inherit (rustLib.commonArgs) pname version;
        };
      };
    };
}
