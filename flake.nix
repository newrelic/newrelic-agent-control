{
  description = "New Relic Agent Control";

  inputs = {
    # Nix package repository
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable"; # Can change this to 25.11 for stable
    # A "framework" of sorts that implements a module system
    flake-parts.url = "github:hercules-ci/flake-parts";
    # Build Rust projects
    crane.url = "github:ipetkov/crane";
    # Manage Rust toolchains
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Git hooks
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Project-wide formatting
    treefmt = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.git-hooks.flakeModule
        inputs.treefmt.flakeModule
      ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      perSystem =
        { system, ... }:
        let
          acLib = import ./nix/lib.nix;
          lib = inputs.nixpkgs.lib;

          nativePkgs = acLib.mkPkgs { inherit inputs system; };

          defaultAgentControl = acLib.mkAgentControlForTarget {
            inherit inputs system;
            # doCheck = false;
          };

          crossPackages = lib.mapAttrs (
            name: target:
            acLib.mkAgentControlForTarget {
              inherit inputs system;
              crossConfig = target;
            }
          ) acLib.defaultCrossTargets;

          windowsPackage = acLib.mkAgentControlForTarget {
            inherit inputs system;
            crossConfig = acLib.windowsCrossConfig;
            windows = true;
          };
        in
        {
          checks = { inherit defaultAgentControl; };
          packages = {
            default = defaultAgentControl;
          }
          // crossPackages
          // lib.optionalAttrs nativePkgs.stdenv.isx86_64 {
            cross-windows = windowsPackage;
          };
          devShells = { };
        };
    };
}
