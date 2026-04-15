{
  description = "New Relic Agent Control - Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-parts.url = "github:hercules-ci/flake-parts";

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
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      # Systems to build for
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      # Import our modular components as flake-parts modules
      imports = [
        ./nix/rust.nix
        ./nix/binary-packages.nix
        ./nix/distro-packages.nix
        ./nix/devshell.nix
      ];

      # Per-system configuration
      perSystem =
        {
          config,
          pkgs,
          system,
          ...
        }:
        {
          # Rust overlay
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import inputs.rust-overlay) ];
          };
        };
    };
}
