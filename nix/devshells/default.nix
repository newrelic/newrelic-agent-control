{
  imports = [
    ./dev.nix
    ./hooks.nix
  ];

  perSystem = { self', ... }: {
    devShells.default = self'.devShells.dev;
  };
}
