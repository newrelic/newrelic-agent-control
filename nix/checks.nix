# `nix flake check`: the formatting and lint steps of CI, runnable anywhere.
# treefmt and git-hooks contribute their own checks from their modules.
_: {
  perSystem =
    { ac, ... }:
    let
      inherit (ac) craneLib;
      workspaceArgs = ac.commonArgs // {
        cargoExtraArgs = "--workspace --all-targets";
      };
    in
    {
      checks = {
        cargo-clippy = craneLib.cargoClippy (
          workspaceArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly workspaceArgs;
            cargoClippyExtraArgs = "-- -D warnings -D clippy::all";
          }
        );

        cargo-fmt = craneLib.cargoFmt {
          inherit (ac) src;
          inherit (ac.commonArgs) pname version;
        };
      };
    };
}
