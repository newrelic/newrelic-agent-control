# Example: Using flake-parts WITHOUT _module.args
# This shows you can use flake-parts in a simpler way

{
  description = "Alternative flake-parts approach without _module.args";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem =
        {
          config,
          pkgs,
          system,
          ...
        }:
        let
          # Apply rust overlay
          pkgs' = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import inputs.rust-overlay) ];
          };

          # Import modules as regular functions (NO _module.args!)
          rust = import ./nix/rust.nix {
            pkgs = pkgs';
            crane = inputs.crane;
            rust-overlay = inputs.rust-overlay;
          };

          binaryPackages = import ./nix/binary-packages.nix {
            pkgs = pkgs';
            inherit rust;
          };

          distroPackages = import ./nix/distro-packages.nix {
            pkgs = pkgs';
            inherit rust;
            binaryPackages = binaryPackages.packages;
          };

          devshell = import ./nix/devshell.nix {
            pkgs = pkgs';
            inherit rust;
          };
        in
        {
          # Expose everything
          packages = binaryPackages.packages // distroPackages.packages;
          devShells.default = devshell;
        };
    };
}

# This approach:
# ✅ Uses flake-parts (gets per-system handling, module system infrastructure)
# ✅ But keeps the simple function-based module style
# ✅ No need to understand _module.args
# ✅ Everything is explicit
#
# Basically: "flake-parts lite" - you get the framework but use it simply
