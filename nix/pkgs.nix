# The package set every perSystem module receives as `pkgs`: nixpkgs plus rust-overlay.
{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    {
      _module.args.pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ inputs.rust-overlay.overlays.default ];
      };
    };
}
