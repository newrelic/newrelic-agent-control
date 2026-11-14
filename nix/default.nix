# Entry point of the flake modules: one directory per kind of flake output.
{
  imports = [
    ./pkgs.nix
    ./ac
    ./packages
    ./devshells
    ./checks
    ./formatting
    ./hooks
  ];
}
