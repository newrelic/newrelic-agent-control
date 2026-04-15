{
  description = "New Relic Agent Control - Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      crane,
      ...
    }:
    let
      # Define the systems we support
      systems = [
        "aarch64-linux"
        "x86_64-linux"
        "aarch64-darwin"
      ];

      # Compute outputs for a single system
      perSystemOutputs = system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };

          # Import modular components
          rust = import ./nix/rust.nix { inherit pkgs crane rust-overlay; };
          binaryPackages = import ./nix/binary-packages.nix { inherit pkgs rust; };
          distroPackages = import ./nix/distro-packages.nix {
            inherit pkgs rust;
            binaryPackages = binaryPackages.packages;
          };
        in
        {
          devShells.default = import ./nix/devshell.nix { inherit pkgs rust; };
          packages = binaryPackages.packages // distroPackages.packages;
        };

      # Generate outputs for all systems
      allOutputs = nixpkgs.lib.genAttrs systems perSystemOutputs;
    in
    {
      # Restructure: outputs.devShells.<system>.default
      devShells = nixpkgs.lib.mapAttrs (_: v: v.devShells) allOutputs;

      # Restructure: outputs.packages.<system>.<package>
      packages = nixpkgs.lib.mapAttrs (_: v: v.packages) allOutputs;
    };
}
