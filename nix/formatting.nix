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

          # YAML only: prettier keeps the blank lines between workflow sections.
          prettier = {
            enable = false;
            includes = [
              "*.yml"
              "*.yaml"
            ];
          };

          rustfmt = {
            enable = true;
            package = rustToolchain;
          };

          shfmt.enable = false;
          taplo.enable = false;
          terraform.enable = false;
          tilt.enable = false;
        };
      };
    };
}
