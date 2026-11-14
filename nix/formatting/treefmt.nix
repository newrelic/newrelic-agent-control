# `nix fmt` and the treefmt check, using the same rustfmt as the pinned toolchain.
{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { config, pkgs, ... }:
    {
      treefmt = {
        projectRootFile = "flake.nix";
        programs.nixfmt.enable = true;
        programs.ruff-format.enable = true;
        programs.rustfmt = {
          enable = true;
          edition = "2024";
          package = pkgs.writeShellScriptBin "rustfmt" ''exec ${config.ac.toolchain}/bin/rustfmt "$@"'';
        };
      };
    };
}
