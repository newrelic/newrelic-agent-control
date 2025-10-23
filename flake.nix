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
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ (import inputs.rust-overlay) ];
          };
          # Retrieve the Rust version from the Cargo.toml file
          rustVersion = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.rust-version;
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
            # Set the toolchain to the pinned Rust version
            p: p.rust-bin.stable.${rustVersion}.default
          );
          # The default AC package definition
          defaultAgentControl = pkgs.callPackage (acLib.crateExpression craneLib) { };
        in
        {
          checks = {
            # Build the crate as part of `nix flake check` for convenience
            inherit defaultAgentControl;
          };
          packages = {
            # Let's start defining everything even though there'll be quite a bit of repetition.
            # First, a default package which builds AC natively for the platform.
            default = defaultAgentControl;
            # Cross-compiled packages
            cross-linux-aarch64-musl =
              let
                crossSystem = {
                  config = "aarch64-unknown-linux-musl";
                };
                localSystem = system;
                pkgs' = import inputs.nixpkgs {
                  inherit crossSystem localSystem;
                  overlays = [ (import inputs.rust-overlay) ];
                };
                craneLib = (inputs.crane.mkLib pkgs').overrideToolchain (
                  p: p.rust-bin.stable.${rustVersion}.default
                );
              in
              pkgs'.callPackage (acLib.crateExpression craneLib) { };

            cross-linux-x86_64-musl =
              let
                crossSystem = {
                  config = "x86_64-unknown-linux-musl";
                };
                localSystem = system;
                pkgs' = import inputs.nixpkgs {
                  inherit crossSystem localSystem;
                  overlays = [ (import inputs.rust-overlay) ];
                };
                craneLib = (inputs.crane.mkLib pkgs').overrideToolchain (
                  p: p.rust-bin.stable.${rustVersion}.default
                );
              in
              pkgs'.callPackage (acLib.crateExpression craneLib) { };

          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isx86_64 {
            # Windows requires `wine64` somewhere, which is not available for aarch64 hosts :(
            # fortunately for us, aarch64-darwin hosts can run these outputs thanks to Rosetta 2!
            cross-windows =
              let
                crossSystem = {
                  config = "x86_64-w64-mingw32";
                  libc = "msvcrt";
                };
                localSystem = system;
                pkgs' = import inputs.nixpkgs {
                  inherit crossSystem localSystem;
                  overlays = [ (import inputs.rust-overlay) ];
                };
                craneLib = (inputs.crane.mkLib pkgs').overrideToolchain (
                  p: p.rust-bin.stable.${rustVersion}.default
                );
              in
              pkgs'.callPackage (acLib.crateExpressionWin craneLib) { };
          };

          devShells = { };
        };
    };
}
