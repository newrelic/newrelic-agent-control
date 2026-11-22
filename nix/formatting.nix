# `nix fmt` and the treefmt check, using the same rustfmt as the pinned toolchain.
{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { config, ... }:
    let
      rustToolchain = config.rustLib.toolchain;
    in
    {
      treefmt = {
        projectRootFile = "flake.nix";
        programs = {
          nixfmt.enable = true;
          taplo.enable = true;
          rustfmt = {
            enable = true;
            package = rustToolchain;
          };
        };
      };
    };
}
