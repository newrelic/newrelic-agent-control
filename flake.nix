{
  description = "New Relic Super Agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    pre-commit-hooks = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    flake-compat.url = "github:edolstra/flake-compat";

    # Rust toolchains
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Compiling Rust projects in cacheable/composable way
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    pre-commit-hooks,
    flake-utils,
    fenix,
    naersk,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};

      rustToolchain = with fenix.packages.${system};
        combine [
          stable.toolchain

          targets.x86_64-unknown-linux-musl.stable.rust-std
          targets.aarch64-unknown-linux-musl.stable.rust-std

          targets.aarch64-apple-darwin.stable.rust-std
          targets.x86_64-apple-darwin.stable.rust-std
        ];

      naersk' = naersk.lib.${system}.override {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      commonArgs = {
        strictDeps = true;
        # Compilation inputs
        buildInputs = with pkgs;
          lib.optionals stdenv.isDarwin [
            libiconv
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];
      };

      newrelic-super-agent = targetPkgs: features: args:
        naersk'.buildPackage (
          commonArgs
          // {
            src = ./.;
            cargoBuildOptions = o: o ++ ["--features=${features}"];

            # Only running test when the build platform is able to run host platform code
            doCheck = with targetPkgs; stdenv.buildPlatform.canExecute stdenv.hostPlatform;
            cargoTestOptions = o: o ++ ["--features=${features}" "-- --skip as_root"];

            CARGO_BUILD_TARGET = targetPkgs.hostPlatform.config;
            # We go static when using the musl target
            CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS = "-C target-feature=+crt-static";
            CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS = "-C target-feature=+crt-static";

            # Linker setups for cross-compilation
            CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = with pkgs.pkgsCross.musl64.stdenv; "${cc}/bin/${cc.targetPrefix}cc";
            CC_x86_64_unknown_linux_musl = with pkgs.pkgsCross.musl64.stdenv; "${cc}/bin/${cc.targetPrefix}cc";
            CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = with pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv; "${cc}/bin/${cc.targetPrefix}cc";
            CC_aarch64_unknown_linux_musl = with pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv; "${cc}/bin/${cc.targetPrefix}cc";
          }
          // pkgs.lib.optionalAttrs (! builtins.isNull args) args
        );
    in {
      checks = {
        pre-commit-check = pre-commit-hooks.lib.${system}.run {
          src = ./.;
          hooks = {
            actionlint.enable = true;
            alejandra.enable = true;
            ansible-lint.enable = false;
            convco.enable = true;
            markdownlint.enable = false;
            rustfmt.enable = true;
            taplo.enable = true;
            # Below is a custom hook.
            third-party-notices = {
              enable = true;
              name = "third-party-notices";
              entry = ''${pkgs.writeShellScriptBin "third-party-notices" ''
                  export LICENSES=$(${pkgs.cargo-deny}/bin/cargo-deny --all-features --manifest-path ./Cargo.toml list -l crate -f json)
                  cargo run --all-features -- --dependencies "$(printf "%s " $LICENSES)" --output-file "./THIRD_PARTY_NOTICES.md"
                  git diff --name-only | grep -q "THIRD_PARTY_NOTICES.md" && { echo "Third party notices out of date, please run \"make -C license third-party-notices\" and commit the changes in this PR.";  exit 1; } || exit 0
                ''}/bin/third-party-notices'';
              # language = "rust";
              pass_filenames = false;
              # For more options, check docs:
              # https://github.com/cachix/pre-commit-hooks.nix#custom-hooks
            };
            cargo-check-onhost = {
              enable = true;
              name = "cargo-check-onhost";
              entry = "${rustToolchain}/bin/cargo check --features onhost";
              language = "rust";
              pass_filenames = false;
            };
            cargo-check-k8s = {
              enable = true;
              name = "cargo-check-k8s";
              entry = "${rustToolchain}/bin/cargo check --features k8s";
              language = "rust";
              pass_filenames = false;
            };
            clippy-onhost = {
              enable = true;
              name = "cippy-onhost";
              entry = "${rustToolchain}/bin/cargo clippy --features onhost";
              language = "rust";
              pass_filenames = false;
            };
            clippy-k8s = {
              enable = true;
              name = "clippy-k8s";
              entry = "${rustToolchain}/bin/cargo clippy --features k8s";
              language = "rust";
              pass_filenames = false;
            };
          };
          tools = {
            cargo = rustToolchain;
            rustfmt = rustToolchain;
            clippy = rustToolchain;
          };
        };
      };

      devShells = {
        default = pkgs.mkShell {
          inherit (self.checks.${system}.pre-commit-check) shellHook;
          buildInputs = with pkgs;
            [
              rustToolchain
              protobuf
              # goreleaser
              # gnumake
            ]
            ++ lib.optionals stdenv.isDarwin [
              libiconv
              darwin.apple_sdk.frameworks.SystemConfiguration
            ];
        };
      };

      packages = {
        # default build, generates outputs native to the host
        default = newrelic-super-agent pkgs "onhost" {
          # Here go additional arguments and overrides
        };

        onhost = newrelic-super-agent pkgs "onhost" null;
        k8s = newrelic-super-agent pkgs "k8s" null;

        # cross x86_64 builds
        x86_64-linux-musl-onhost = newrelic-super-agent pkgs.pkgsCross.musl64 "onhost" null;
        x86_64-linux-musl-k8s = newrelic-super-agent pkgs.pkgsCross.musl64 "k8s" null;
        # cross aarch64 builds
        aarch64-linux-musl-onhost = newrelic-super-agent pkgs.pkgsCross.aarch64-multiplatform-musl "onhost" null;
        aarch64-linux-musl-k8s = newrelic-super-agent pkgs.pkgsCross.aarch64-multiplatform-musl "k8s" null;
      };
    });
}
